use std::{collections::HashMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use tokio::{
    fs::OpenOptions,
    process::{Child, Command},
    sync::{Mutex, RwLock},
    time::sleep,
};

use crate::{
    ActionResult, ConfigStore, OllamaClient, Result, ServiceKind, ServiceSnapshot, ServiceState,
    StackConfig, StackError, StackSnapshot, inspect_environment,
};

#[derive(Clone)]
pub struct StackSupervisor {
    store: ConfigStore,
    config: Arc<RwLock<StackConfig>>,
    children: Arc<Mutex<HashMap<ServiceKind, Child>>>,
    http: reqwest::Client,
}

impl StackSupervisor {
    pub async fn discover() -> Result<Self> {
        let store = ConfigStore::discover()?;
        Self::new(store).await
    }

    pub async fn new(store: ConfigStore) -> Result<Self> {
        let config = store.load().await?;
        Ok(Self {
            store,
            config: Arc::new(RwLock::new(config)),
            children: Arc::new(Mutex::new(HashMap::new())),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()?,
        })
    }

    pub async fn config(&self) -> StackConfig {
        self.config.read().await.clone()
    }

    pub async fn save_config(&self, config: StackConfig) -> Result<ActionResult> {
        self.store.save(&config).await?;
        *self.config.write().await = config;
        Ok(ActionResult::success("Configuration saved"))
    }

    pub async fn snapshot(&self) -> Result<StackSnapshot> {
        self.reap_exited().await;
        let config = self.config().await;
        let ollama_client = OllamaClient::new(&config.ollama.url)?;

        let (ollama_version, harness_online) = tokio::join!(
            ollama_client.version(),
            self.is_online(ServiceKind::Harness, &config.harness.url)
        );

        let ollama_online = ollama_version.is_ok();

        let managed = self.managed_processes().await;
        let installed_models = if ollama_online {
            ollama_client.installed_models().await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let running_models = if ollama_online {
            ollama_client.running_models().await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let environment = inspect_environment().await;

        Ok(StackSnapshot {
            ollama: ServiceSnapshot {
                kind: ServiceKind::Ollama,
                state: if ollama_online {
                    ServiceState::Online
                } else {
                    ServiceState::Offline
                },
                url: config.ollama.url,
                version: ollama_version.ok(),
                managed: managed.contains_key(&ServiceKind::Ollama),
                pid: managed.get(&ServiceKind::Ollama).copied(),
                message: None,
            },
            harness: ServiceSnapshot {
                kind: ServiceKind::Harness,
                state: if harness_online {
                    ServiceState::Online
                } else {
                    ServiceState::Offline
                },
                url: config.harness.url,
                version: None,
                managed: managed.contains_key(&ServiceKind::Harness),
                pid: managed.get(&ServiceKind::Harness).copied(),
                message: None,
            },
            installed_models,
            running_models,
            environment,
            config_path: self.store.path().display().to_string(),
        })
    }

    pub async fn start(&self, kind: ServiceKind) -> Result<ActionResult> {
        self.reap_exited().await;
        let snapshot = self.snapshot().await?;
        let online = match kind {
            ServiceKind::Ollama => snapshot.ollama.state == ServiceState::Online,
            ServiceKind::Harness => snapshot.harness.state == ServiceState::Online,
        };
        if online {
            return Err(StackError::AlreadyRunning(kind.to_string()));
        }

        let config = self.config().await;
        let service = match kind {
            ServiceKind::Ollama => config.ollama,
            ServiceKind::Harness => config.harness,
        };
        let (executable, args) = resolve_launch(kind, service.command.as_deref(), &service.args)?;
        let log_path = self.log_path(kind).await?;
        let stdout = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await?;
        let stderr = stdout.try_clone().await?;

        let mut command = platform_command(&executable, &args);
        command
            .stdin(Stdio::null())
            .stdout(stdout.into_std().await)
            .stderr(stderr.into_std().await);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);

        let child = command.spawn()?;
        let pid = child.id();
        self.children.lock().await.insert(kind, child);

        for _ in 0..20 {
            sleep(Duration::from_millis(400)).await;
            if self.is_online(kind, &service.url).await {
                return Ok(ActionResult::success(format!(
                    "{kind} started (PID {})",
                    pid.unwrap_or_default()
                )));
            }
        }

        Ok(ActionResult::success(format!(
            "{kind} process started; health endpoint is still warming up"
        )))
    }

    pub async fn stop(&self, kind: ServiceKind) -> Result<ActionResult> {
        let mut children = self.children.lock().await;
        let mut child = children
            .remove(&kind)
            .ok_or_else(|| StackError::NotManaged(kind.to_string()))?;
        child.kill().await?;
        let _ = child.wait().await;
        Ok(ActionResult::success(format!("{kind} stopped")))
    }

    pub async fn restart(&self, kind: ServiceKind) -> Result<ActionResult> {
        if self.children.lock().await.contains_key(&kind) {
            self.stop(kind).await?;
            sleep(Duration::from_millis(500)).await;
        } else {
            let snapshot = self.snapshot().await?;
            let online = match kind {
                ServiceKind::Ollama => snapshot.ollama.state == ServiceState::Online,
                ServiceKind::Harness => snapshot.harness.state == ServiceState::Online,
            };
            if online {
                return Err(StackError::NotManaged(kind.to_string()));
            }
        }
        self.start(kind).await
    }

    pub async fn pull_model(&self, model: &str) -> Result<ActionResult> {
        let config = self.config().await;
        OllamaClient::new(config.ollama.url)?
            .pull_model(model)
            .await?;
        Ok(ActionResult::success(format!("Downloaded {model}")))
    }

    pub async fn unload_model(&self, model: &str) -> Result<ActionResult> {
        let config = self.config().await;
        OllamaClient::new(config.ollama.url)?
            .unload_model(model)
            .await?;
        Ok(ActionResult::success(format!(
            "Released GPU memory used by {model}"
        )))
    }

    pub async fn delete_model(&self, model: &str) -> Result<ActionResult> {
        let config = self.config().await;
        OllamaClient::new(config.ollama.url)?
            .delete_model(model)
            .await?;
        Ok(ActionResult::success(format!("Deleted {model}")))
    }

    async fn managed_processes(&self) -> HashMap<ServiceKind, u32> {
        self.children
            .lock()
            .await
            .iter()
            .filter_map(|(kind, child)| child.id().map(|pid| (*kind, pid)))
            .collect()
    }

    async fn reap_exited(&self) {
        let mut children = self.children.lock().await;
        let exited: Vec<_> = children
            .iter_mut()
            .filter_map(|(kind, child)| match child.try_wait() {
                Ok(Some(_)) => Some(*kind),
                _ => None,
            })
            .collect();
        for kind in exited {
            children.remove(&kind);
        }
    }

    async fn log_path(&self, kind: ServiceKind) -> Result<PathBuf> {
        let parent = self
            .store
            .path()
            .parent()
            .ok_or_else(|| StackError::Config("configuration path has no parent".into()))?;
        let logs = parent.join("logs");
        tokio::fs::create_dir_all(&logs).await?;
        Ok(logs.join(format!("{}.log", kind.to_string().to_lowercase())))
    }

    async fn is_online(&self, kind: ServiceKind, url: &str) -> bool {
        match kind {
            ServiceKind::Ollama => match OllamaClient::new(url) {
                Ok(client) => client.version().await.is_ok(),
                Err(_) => false,
            },
            ServiceKind::Harness => self
                .http
                .get(url)
                .send()
                .await
                .map(|response| {
                    response.status().is_success() || response.status().is_redirection()
                })
                .unwrap_or(false),
        }
    }
}

fn resolve_launch(
    kind: ServiceKind,
    configured: Option<&str>,
    args: &[String],
) -> Result<(PathBuf, Vec<String>)> {
    if let Some(value) = configured.filter(|value| !value.trim().is_empty()) {
        let path = PathBuf::from(value);
        let resolved = if path.is_file() {
            path
        } else {
            which::which(value).map_err(|_| StackError::CommandNotFound {
                service: kind.to_string(),
            })?
        };
        return Ok((resolved, args.to_vec()));
    }

    let name = match kind {
        ServiceKind::Ollama => "ollama",
        ServiceKind::Harness => "dsh",
    };
    if let Ok(path) = which::which(name) {
        return Ok((path, args.to_vec()));
    }

    if kind == ServiceKind::Harness
        && let Ok(path) = which::which("npx")
    {
        let mut npx_args = vec!["--yes".into(), "@deepseek-ai/dsh".into()];
        npx_args.extend_from_slice(args);
        return Ok((path, npx_args));
    }

    #[cfg(windows)]
    if kind == ServiceKind::Ollama
        && let Some(local) = std::env::var_os("LOCALAPPDATA")
    {
        let path = PathBuf::from(local).join("Programs/Ollama/ollama.exe");
        if path.is_file() {
            return Ok((path, args.to_vec()));
        }
    }

    Err(StackError::CommandNotFound {
        service: kind.to_string(),
    })
}

fn platform_command(executable: &PathBuf, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        let extension = executable
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
            let mut command = Command::new("cmd.exe");
            command
                .arg("/d")
                .arg("/s")
                .arg("/c")
                .arg(executable)
                .args(args);
            return command;
        }
        if extension.eq_ignore_ascii_case("ps1") {
            let mut command = Command::new("powershell.exe");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(executable)
                .args(args);
            return command;
        }
    }

    let mut command = Command::new(executable);
    command.args(args);
    command
}
