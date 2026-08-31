use std::{
    collections::{HashMap, VecDeque},
    io::SeekFrom,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use directories::BaseDirs;
use tokio::{
    fs::{self, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt},
    process::{Child, Command},
    sync::{Mutex, RwLock},
    time::{sleep, timeout},
};

use crate::{
    ActionResult, ConfigStore, ManagedRuntimeStore, OllamaClient, PullProgress, Result,
    RuntimeInstallProgress, ServiceKind, ServiceLogTail, ServiceSnapshot, ServiceState,
    StackConfig, StackError, StackSnapshot, assess_versions, download_and_extract_verified,
    embedded_artifact, embedded_manifest, export_diagnostics, inspect_environment,
};

const HARNESS_COMPANION_URL: &str = "https://github.com/HanyangZZZ/local-agent-stack/releases/download/v0.1.0-alpha.2/local-agent-stack-harness-companion-0.1.0-alpha.2.tgz";
const LOG_TAIL_MAX_BYTES: u64 = 256 * 1024;
const LOG_TAIL_MAX_LINES: usize = 250;

#[derive(Clone)]
pub struct StackSupervisor {
    store: ConfigStore,
    config: Arc<RwLock<StackConfig>>,
    children: Arc<Mutex<HashMap<ServiceKind, Child>>>,
    http: reqwest::Client,
    download_http: reqwest::Client,
    managed_runtimes: ManagedRuntimeStore,
}

impl StackSupervisor {
    pub async fn discover() -> Result<Self> {
        let store = ConfigStore::discover()?;
        Self::new(store).await
    }

    pub async fn new(store: ConfigStore) -> Result<Self> {
        let config = store.load().await?;
        let managed_runtimes = ManagedRuntimeStore::discover()?;
        for kind in [ServiceKind::Ollama, ServiceKind::Harness] {
            let _ = managed_runtimes
                .cleanup_staging_older_than(kind, Duration::from_secs(24 * 60 * 60))
                .await;
        }
        Ok(Self {
            store,
            config: Arc::new(RwLock::new(config)),
            children: Arc::new(Mutex::new(HashMap::new())),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()?,
            download_http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .build()?,
            managed_runtimes,
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

    pub async fn complete_setup(&self) -> Result<ActionResult> {
        let mut config = self.config().await;
        config.setup_completed = true;
        self.store.save(&config).await?;
        *self.config.write().await = config;
        Ok(ActionResult::success("First-run setup completed"))
    }

    pub async fn export_diagnostic_report(&self) -> Result<ActionResult> {
        let config = self.config().await;
        let snapshot = self.snapshot().await?;
        let fallback = self
            .store
            .path()
            .parent()
            .ok_or_else(|| StackError::Config("configuration path has no parent".into()))?;
        let path = export_diagnostics(snapshot, &config, fallback).await?;
        Ok(ActionResult::success(format!(
            "Diagnostics exported to {}",
            path.display()
        )))
    }

    pub async fn service_log_tail(&self, kind: ServiceKind) -> Result<ServiceLogTail> {
        read_service_log_tail(&self.log_path(kind).await?, kind).await
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
        let ollama_version = ollama_version.ok();

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
        let mut environment = inspect_environment().await;
        if config.ollama.command.is_some() {
            environment.ollama_path = config.ollama.command.clone();
        }
        if config.harness.command.is_some() {
            environment.harness_path = config.harness.command.clone();
        }
        if environment.node_path.is_none()
            && let Some(command) = config.harness.command.as_deref()
            && let Some(parent) = Path::new(command).parent()
        {
            let node = parent.join(if cfg!(windows) { "node.exe" } else { "node" });
            if node.is_file() {
                environment.node_path = Some(node.display().to_string());
            }
        }
        let harness_version = match (
            environment.harness_path.as_deref(),
            config.managed_harness_entrypoint.as_deref(),
        ) {
            (Some(command), Some(entrypoint)) => {
                inspect_managed_harness_version(command, entrypoint)
                    .await
                    .ok()
            }
            (Some(command), None) if is_harness_command(command) => {
                inspect_cli_version(command).await
            }
            _ => None,
        };
        let compatibility = assess_versions(ollama_version.as_deref(), harness_version.as_deref())?;
        let managed_ollama = self.managed_runtimes.status(ServiceKind::Ollama).await?;
        let managed_harness = self.managed_runtimes.status(ServiceKind::Harness).await?;

        Ok(StackSnapshot {
            ollama: ServiceSnapshot {
                kind: ServiceKind::Ollama,
                state: if ollama_online {
                    ServiceState::Online
                } else {
                    ServiceState::Offline
                },
                url: config.ollama.url,
                version: ollama_version,
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
                version: harness_version,
                managed: managed.contains_key(&ServiceKind::Harness),
                pid: managed.get(&ServiceKind::Harness).copied(),
                message: None,
            },
            installed_models,
            running_models,
            environment,
            compatibility,
            managed_ollama,
            managed_harness,
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
        let harness_home = config.harness_home.clone();
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
        if kind == ServiceKind::Harness
            && let Some(home) = harness_home
        {
            command.env("DSH_HOME", home);
        }
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
        let mut child = self
            .children
            .lock()
            .await
            .remove(&kind)
            .ok_or_else(|| StackError::NotManaged(kind.to_string()))?;
        if let Err(error) = child.kill().await {
            self.children.lock().await.insert(kind, child);
            return Err(error.into());
        }
        let _ = child.wait().await;
        Ok(ActionResult::success(format!("{kind} stopped")))
    }

    pub async fn start_stack(&self) -> Result<ActionResult> {
        let snapshot = self.snapshot().await?;
        let ollama_online = snapshot.ollama.state == ServiceState::Online;
        let harness_online = snapshot.harness.state == ServiceState::Online;
        let plan = stack_start_plan(ollama_online, harness_online);
        if plan.is_empty() {
            return Ok(ActionResult::success(
                "The local agent stack is already online",
            ));
        }

        let mut started = Vec::new();
        for kind in plan {
            if let Err(error) = self.start(kind).await {
                let mut rollback_failures = Vec::new();
                for rollback_kind in started.iter().rev().copied() {
                    if let Err(rollback_error) = self.stop(rollback_kind).await {
                        rollback_failures.push(format!("{rollback_kind}: {rollback_error}"));
                    }
                }
                let rollback = if rollback_failures.is_empty() {
                    "services started by this action were stopped".to_owned()
                } else {
                    format!(
                        "rollback needs attention ({})",
                        rollback_failures.join(", ")
                    )
                };
                return Err(StackError::Config(format!(
                    "could not start {kind}: {error}; {rollback}"
                )));
            }
            started.push(kind);
        }

        let names = started
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" and ");
        let message = if ollama_online || harness_online {
            format!("Stack ready · started {names} · kept existing services online")
        } else {
            format!("Stack ready · started {names}")
        };
        Ok(ActionResult::success(message))
    }

    pub async fn stop_managed_stack(&self) -> Result<ActionResult> {
        self.reap_exited().await;
        let managed = self.managed_processes().await;
        let plan = stack_stop_plan(
            managed.contains_key(&ServiceKind::Ollama),
            managed.contains_key(&ServiceKind::Harness),
        );
        if plan.is_empty() {
            return Ok(ActionResult::success(
                "No app-managed services are running; external services were left alone",
            ));
        }

        let mut stopped = Vec::new();
        let mut failures = Vec::new();
        for kind in plan {
            match self.stop(kind).await {
                Ok(_) => stopped.push(kind),
                Err(error) => failures.push(format!("{kind}: {error}")),
            }
        }
        if !failures.is_empty() {
            return Err(StackError::Config(format!(
                "some app-managed services could not be stopped: {}",
                failures.join(", ")
            )));
        }

        let names = stopped
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" and ");
        Ok(ActionResult::success(format!(
            "Stopped app-managed {names} · external services were left alone"
        )))
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
        self.pull_model_with_progress(model, |_| {}).await
    }

    pub async fn pull_model_with_progress<F>(
        &self,
        model: &str,
        on_progress: F,
    ) -> Result<ActionResult>
    where
        F: FnMut(PullProgress) + Send,
    {
        let config = self.config().await;
        OllamaClient::new(config.ollama.url)?
            .pull_model_with_progress(model, on_progress)
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

    pub async fn unload_all_models(&self) -> Result<ActionResult> {
        let config = self.config().await;
        let count = OllamaClient::new(config.ollama.url)?
            .unload_all_models()
            .await?;
        let message = match count {
            0 => "No Ollama models are currently using GPU memory".into(),
            1 => "Released GPU memory used by 1 Ollama model".into(),
            count => format!("Released GPU memory used by {count} Ollama models"),
        };
        Ok(ActionResult::success(message))
    }

    pub async fn delete_model(&self, model: &str) -> Result<ActionResult> {
        let config = self.config().await;
        OllamaClient::new(config.ollama.url)?
            .delete_model(model)
            .await?;
        Ok(ActionResult::success(format!("Deleted {model}")))
    }

    pub async fn install_managed_ollama_with_progress<F>(
        &self,
        mut on_progress: F,
    ) -> Result<ActionResult>
    where
        F: FnMut(RuntimeInstallProgress) + Send,
    {
        let artifact = embedded_artifact(
            ServiceKind::Ollama,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )?;
        let version = artifact.version.to_string();
        let staging = self
            .managed_runtimes
            .create_staging_directory(ServiceKind::Ollama, &version)
            .await?;
        let result = async {
            let extracted = download_and_extract_verified(
                &self.download_http,
                &artifact,
                &staging,
                &mut on_progress,
            )
            .await?;
            let detected = inspect_ollama_executable(&extracted).await?;
            if !detected.contains(&version) {
                return Err(StackError::Config(format!(
                    "the staged Ollama reported {detected}, expected {version}"
                )));
            }
            on_progress(RuntimeInstallProgress {
                kind: ServiceKind::Ollama,
                stage: "activating".into(),
                completed: artifact.download_size,
                total: artifact.download_size,
                message: "Activating the verified Ollama release".into(),
            });
            self.managed_runtimes
                .activate_staging(
                    ServiceKind::Ollama,
                    &version,
                    &staging,
                    Path::new(&artifact.executable_relative_path),
                )
                .await
        }
        .await;

        let executable = match result {
            Ok(executable) => executable,
            Err(error) => {
                let _ = self
                    .managed_runtimes
                    .abandon_staging(ServiceKind::Ollama, &staging)
                    .await;
                return Err(error);
            }
        };
        let mut config = self.config().await;
        config.ollama.command = Some(executable.display().to_string());
        self.store.save(&config).await?;
        *self.config.write().await = config;
        Ok(ActionResult::success(format!(
            "Installed and activated verified app-owned Ollama {version}; the previous managed release was preserved"
        )))
    }

    pub async fn rollback_managed_ollama(&self) -> Result<ActionResult> {
        let executable = self.managed_runtimes.rollback(ServiceKind::Ollama).await?;
        let detected = inspect_ollama_executable(&executable).await?;
        let mut config = self.config().await;
        config.ollama.command = Some(executable.display().to_string());
        self.store.save(&config).await?;
        *self.config.write().await = config;
        Ok(ActionResult::success(format!(
            "Rolled back the app-owned Ollama to {detected}"
        )))
    }

    pub async fn prepare_harness_profile(&self) -> Result<ActionResult> {
        let mut config = self.config().await;
        validate_profile_name(&config.harness_profile)?;
        let home = harness_home(&config)?;
        let profiles = home.join("profiles");
        let source = profiles.join("web");
        let destination = profiles.join(&config.harness_profile);

        if !source.join("package.json").is_file() {
            return Err(StackError::Config(format!(
                "the source Harness web profile was not found at {}",
                source.display()
            )));
        }

        let validation_message = if destination.is_dir() {
            self.validate_harness_profile(&config, &home, &config.harness_profile)
                .await?;
            format!("Harness profile {} is valid", config.harness_profile)
        } else {
            fs::create_dir_all(&profiles).await?;
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| StackError::Config(error.to_string()))?
                .as_millis();
            let temporary_name = format!(".las-{}-{nonce}", config.harness_profile);
            let temporary = profiles.join(&temporary_name);
            fs::create_dir(&temporary).await?;

            let result = async {
                copy_profile_file(&source, &temporary, "cordis.patch.yml", false).await?;
                copy_profile_file(&source, &temporary, "pnpm-workspace.yaml", false).await?;
                let package = fs::read(source.join("package.json")).await?;
                let mut package: serde_json::Value = serde_json::from_slice(&package)?;
                package["name"] =
                    serde_json::Value::String(format!("dsh-profile-{}", config.harness_profile));
                fs::write(
                    temporary.join("package.json"),
                    serde_json::to_vec_pretty(&package)?,
                )
                .await?;

                self.validate_harness_profile(&config, &home, &temporary_name)
                    .await?;
                fs::rename(&temporary, &destination).await?;
                Result::<()>::Ok(())
            }
            .await;

            if result.is_err() && temporary.is_dir() {
                let _ = fs::remove_dir_all(&temporary).await;
            }
            result?;
            format!(
                "Created and validated Harness profile {}",
                config.harness_profile
            )
        };

        config.harness_home = Some(home.display().to_string());
        config.harness.args = harness_profile_args(&config)?;
        self.store.save(&config).await?;
        *self.config.write().await = config;
        Ok(ActionResult::success(validation_message))
    }

    pub async fn install_harness_companion(&self) -> Result<ActionResult> {
        let config = self.config().await;
        validate_profile_name(&config.harness_profile)?;
        let home = harness_home(&config)?;
        let profile = home.join("profiles").join(&config.harness_profile);
        if !profile.is_dir() {
            return Err(StackError::Config(
                "prepare the managed Harness profile before installing its companion".into(),
            ));
        }

        let args = harness_cli_args(
            &config,
            vec![
                "plugin".into(),
                "--profile".into(),
                config.harness_profile.clone(),
                "add".into(),
                HARNESS_COMPANION_URL.into(),
            ],
        );
        let (executable, args) = resolve_launch(
            ServiceKind::Harness,
            config.harness.command.as_deref(),
            &args,
        )?;
        let mut command = platform_command(&executable, &args);
        command
            .env("DSH_HOME", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);

        let output = timeout(Duration::from_secs(180), command.output())
            .await
            .map_err(|_| StackError::Config("Harness companion installation timed out".into()))??;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(StackError::Config(format!(
                "Harness companion installation failed: {} {}",
                stdout.trim(),
                stderr.trim()
            )));
        }

        self.validate_harness_profile(&config, &home, &config.harness_profile)
            .await?;
        Ok(ActionResult::success(format!(
            "Installed and validated the Harness companion in {}",
            config.harness_profile
        )))
    }

    pub async fn install_managed_harness(&self) -> Result<ActionResult> {
        let mut config = self.config().await;
        let requirement = embedded_manifest()?
            .components
            .into_iter()
            .find(|component| component.kind == ServiceKind::Harness)
            .ok_or_else(|| {
                StackError::Config("the compatibility manifest has no Harness release".into())
            })?;
        let version = requirement.recommended_version.to_string();
        let node = resolve_managed_tool(
            config.managed_harness_node.as_deref(),
            config.harness.command.as_deref(),
            if cfg!(windows) { "node.exe" } else { "node" },
        )?;
        let source = resolve_harness_package_source(
            config.managed_harness_source.as_deref(),
            config.harness.command.as_deref(),
        )?;
        validate_harness_package(&source, &version).await?;
        let staging = self
            .managed_runtimes
            .create_staging_directory(ServiceKind::Harness, &version)
            .await?;

        let result = async {
            let executable_relative = PathBuf::from(managed_node_name());
            let executable = staging.join(&executable_relative);
            let entrypoint = staging.join("package").join("lib").join("bin.js");
            copy_directory_tree(&source, &staging.join("package")).await?;
            fs::copy(&node, &executable).await?;
            let detected = inspect_managed_harness_version(
                executable.to_string_lossy().as_ref(),
                entrypoint.to_string_lossy().as_ref(),
            )
            .await?;
            if detected.trim_start_matches('v') != version {
                return Err(StackError::Config(format!(
                    "the staged Harness reported {detected}, expected {version}"
                )));
            }

            self.managed_runtimes
                .activate_staging(
                    ServiceKind::Harness,
                    &version,
                    &staging,
                    &executable_relative,
                )
                .await
        }
        .await;

        let executable = match result {
            Ok(executable) => executable,
            Err(error) => {
                let _ = self
                    .managed_runtimes
                    .abandon_staging(ServiceKind::Harness, &staging)
                    .await;
                return Err(error);
            }
        };

        config.harness.command = Some(executable.display().to_string());
        config.managed_harness_entrypoint = Some(
            executable
                .parent()
                .ok_or_else(|| StackError::Config("managed Harness release has no parent".into()))?
                .join("package")
                .join("lib")
                .join("bin.js")
                .display()
                .to_string(),
        );
        config.managed_harness_node = Some(node.display().to_string());
        config.managed_harness_source = Some(source.display().to_string());
        config.harness.args = harness_profile_args(&config)?;
        self.store.save(&config).await?;
        *self.config.write().await = config;
        Ok(ActionResult::success(format!(
            "Imported and activated app-owned Harness {version}; the external installation remains unchanged and the previous managed release was preserved"
        )))
    }

    pub async fn rollback_managed_harness(&self) -> Result<ActionResult> {
        let executable = self.managed_runtimes.rollback(ServiceKind::Harness).await?;
        let entrypoint = executable
            .parent()
            .ok_or_else(|| StackError::Config("managed Harness release has no parent".into()))?
            .join("package")
            .join("lib")
            .join("bin.js");
        let detected = inspect_managed_harness_version(
            executable.to_string_lossy().as_ref(),
            entrypoint.to_string_lossy().as_ref(),
        )
        .await?;
        let mut config = self.config().await;
        config.harness.command = Some(executable.display().to_string());
        config.managed_harness_entrypoint = Some(entrypoint.display().to_string());
        config.harness.args = harness_profile_args(&config)?;
        self.store.save(&config).await?;
        *self.config.write().await = config;
        Ok(ActionResult::success(format!(
            "Rolled back the app-owned Harness to {detected}"
        )))
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

    async fn validate_harness_profile(
        &self,
        config: &StackConfig,
        home: &Path,
        profile: &str,
    ) -> Result<()> {
        let args = harness_cli_args(
            config,
            vec!["--profile".into(), profile.into(), "--dump-config".into()],
        );
        let (executable, args) = resolve_launch(
            ServiceKind::Harness,
            config.harness.command.as_deref(),
            &args,
        )?;
        let mut command = platform_command(&executable, &args);
        command
            .env("DSH_HOME", home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);

        let output = timeout(Duration::from_secs(45), command.output())
            .await
            .map_err(|_| StackError::Config("Harness profile validation timed out".into()))??;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            return Err(StackError::Config(format!(
                "Harness rejected profile {profile}: {}",
                detail.trim()
            )));
        }
        if output.stdout.is_empty() {
            return Err(StackError::Config(format!(
                "Harness profile {profile} produced an empty configuration"
            )));
        }
        Ok(())
    }
}

fn harness_home(config: &StackConfig) -> Result<PathBuf> {
    if let Some(value) = config
        .harness_home
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(value));
    }
    if let Some(value) = std::env::var_os("DSH_HOME") {
        return Ok(PathBuf::from(value));
    }
    let base = BaseDirs::new()
        .ok_or_else(|| StackError::Config("the user home directory is unavailable".into()))?;
    let default = base.home_dir().join(".dsh");
    if default.is_dir() {
        return Ok(default);
    }
    Err(StackError::Config(
        "Harness home was not detected; set it in Settings".into(),
    ))
}

fn validate_profile_name(profile: &str) -> Result<()> {
    let valid = !profile.is_empty()
        && profile.len() <= 64
        && profile
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'))
        && profile != "."
        && profile != "..";
    if valid {
        Ok(())
    } else {
        Err(StackError::Config(
            "Harness profile must contain only letters, numbers, dots, dashes, or underscores"
                .into(),
        ))
    }
}

async fn copy_profile_file(
    source: &Path,
    destination: &Path,
    name: &str,
    required: bool,
) -> Result<()> {
    let source = source.join(name);
    if source.is_file() {
        fs::copy(source, destination.join(name)).await?;
    } else if required {
        return Err(StackError::Config(format!(
            "Harness profile file {name} is missing"
        )));
    }
    Ok(())
}

fn harness_profile_args(config: &StackConfig) -> Result<Vec<String>> {
    let url = reqwest::Url::parse(&config.harness.url)
        .map_err(|error| StackError::Config(format!("Harness URL is invalid: {error}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| StackError::Config("Harness URL has no host".into()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| StackError::Config("Harness URL has no port".into()))?;
    Ok(harness_cli_args(
        config,
        vec![
            "--profile".into(),
            config.harness_profile.clone(),
            "--host".into(),
            host.into(),
            "--port".into(),
            port.to_string(),
            "--no-open".into(),
        ],
    ))
}

fn harness_cli_args(config: &StackConfig, args: Vec<String>) -> Vec<String> {
    match config.managed_harness_entrypoint.as_ref() {
        Some(entrypoint) => std::iter::once(entrypoint.clone()).chain(args).collect(),
        None => args,
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

fn resolve_managed_tool(
    configured: Option<&str>,
    harness_command: Option<&str>,
    name: &str,
) -> Result<PathBuf> {
    if let Some(path) = configured.map(PathBuf::from).filter(|path| path.is_file()) {
        return Ok(path);
    }
    if let Some(parent) = harness_command.and_then(|command| Path::new(command).parent()) {
        let sibling = parent.join(name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    which::which(name).map_err(|_| StackError::CommandNotFound {
        service: format!("Harness installation tool {name}"),
    })
}

fn managed_node_name() -> &'static str {
    if cfg!(windows) { "node.exe" } else { "node" }
}

fn resolve_harness_package_source(
    configured: Option<&str>,
    harness_command: Option<&str>,
) -> Result<PathBuf> {
    if let Some(path) = configured.map(PathBuf::from).filter(|path| path.is_dir()) {
        return Ok(path);
    }
    let command = harness_command
        .map(PathBuf::from)
        .or_else(|| which::which("dsh").ok())
        .ok_or_else(|| StackError::CommandNotFound {
            service: "Harness import source".into(),
        })?;
    let parent = command.parent().ok_or_else(|| {
        StackError::Config("the configured Harness executable has no parent".into())
    })?;
    for candidate in [
        parent.join("package"),
        parent.join("node_modules").join("@deepseek-ai").join("dsh"),
    ] {
        if candidate.join("package.json").is_file() {
            return Ok(candidate);
        }
    }
    Err(StackError::Config(
        "the installed @deepseek-ai/dsh package could not be located next to the configured executable"
            .into(),
    ))
}

async fn validate_harness_package(source: &Path, expected_version: &str) -> Result<()> {
    let bytes = fs::read(source.join("package.json")).await?;
    let package: serde_json::Value = serde_json::from_slice(&bytes)?;
    let name = package.get("name").and_then(|value| value.as_str());
    let version = package.get("version").and_then(|value| value.as_str());
    if name != Some("@deepseek-ai/dsh") {
        return Err(StackError::Config(
            "the Harness import source is not the @deepseek-ai/dsh package".into(),
        ));
    }
    if version != Some(expected_version) {
        return Err(StackError::Config(format!(
            "installed Harness is {}, but the tested release is {expected_version}; update the external Harness before importing it",
            version.unwrap_or("unknown")
        )));
    }
    if !source.join("lib").join("bin.js").is_file() {
        return Err(StackError::Config(
            "the Harness import source is missing lib/bin.js".into(),
        ));
    }
    Ok(())
}

async fn copy_directory_tree(source: &Path, destination: &Path) -> Result<()> {
    let canonical_source = fs::canonicalize(source).await?;
    fs::create_dir_all(destination).await?;
    let mut pending = VecDeque::from([(canonical_source, destination.to_path_buf())]);
    while let Some((current_source, current_destination)) = pending.pop_front() {
        let mut entries = fs::read_dir(&current_source).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            let target = current_destination.join(entry.file_name());
            if file_type.is_dir() {
                fs::create_dir(&target).await?;
                pending.push_back((entry.path(), target));
            } else if file_type.is_file() {
                fs::copy(entry.path(), target).await?;
            } else {
                return Err(StackError::Config(format!(
                    "Harness import source contains an unsupported link or special file: {}",
                    entry.path().display()
                )));
            }
        }
    }
    Ok(())
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

fn is_harness_command(value: &str) -> bool {
    Path::new(value)
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("dsh"))
}

async fn inspect_cli_version(value: &str) -> Option<String> {
    inspect_cli_version_result(value).await.ok()
}

async fn inspect_cli_version_result(value: &str) -> Result<String> {
    let executable = PathBuf::from(value);
    let mut command = platform_command(&executable, &["--version".into()]);
    command.kill_on_drop(true);
    let output = timeout(Duration::from_secs(3), command.output())
        .await
        .map_err(|_| StackError::Config("Harness version check timed out".into()))??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(StackError::Config(format!(
            "Harness version check failed: {}",
            stderr.trim()
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| StackError::Config("Harness version check returned no version".into()))
}

async fn inspect_managed_harness_version(node: &str, entrypoint: &str) -> Result<String> {
    let mut command = Command::new(node);
    command.arg(entrypoint).arg("--version").kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = timeout(Duration::from_secs(3), command.output())
        .await
        .map_err(|_| StackError::Config("managed Harness version check timed out".into()))??;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(StackError::Config(format!(
            "managed Harness version check failed: {}",
            detail.trim()
        )));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            StackError::Config("managed Harness version check returned no version".into())
        })
}

async fn inspect_ollama_executable(executable: &Path) -> Result<String> {
    let mut command = Command::new(executable);
    command.arg("--version").kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| StackError::Config("Ollama version check timed out".into()))??;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(StackError::Config(format!(
            "Ollama version check failed: {}",
            detail.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detected = format!("{} {}", stdout.trim(), stderr.trim())
        .trim()
        .to_owned();
    if detected.is_empty() {
        return Err(StackError::Config(
            "Ollama version check returned no version".into(),
        ));
    }
    Ok(detected)
}

fn stack_start_plan(ollama_online: bool, harness_online: bool) -> Vec<ServiceKind> {
    [
        (!ollama_online).then_some(ServiceKind::Ollama),
        (!harness_online).then_some(ServiceKind::Harness),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn stack_stop_plan(ollama_managed: bool, harness_managed: bool) -> Vec<ServiceKind> {
    [
        harness_managed.then_some(ServiceKind::Harness),
        ollama_managed.then_some(ServiceKind::Ollama),
    ]
    .into_iter()
    .flatten()
    .collect()
}

async fn read_service_log_tail(path: &Path, kind: ServiceKind) -> Result<ServiceLogTail> {
    let metadata = match fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ServiceLogTail {
                kind,
                content: String::new(),
                source_bytes: 0,
                line_count: 0,
                truncated: false,
                exists: false,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let source_bytes = metadata.len();
    let start = source_bytes.saturating_sub(LOG_TAIL_MAX_BYTES);
    let mut file = fs::File::open(path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    let mut bytes = Vec::with_capacity((source_bytes - start).min(LOG_TAIL_MAX_BYTES) as usize);
    file.take(LOG_TAIL_MAX_BYTES)
        .read_to_end(&mut bytes)
        .await?;

    let mut truncated = start > 0;
    if start > 0
        && let Some(first_newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=first_newline);
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines().collect::<Vec<_>>();
    if lines.len() > LOG_TAIL_MAX_LINES {
        let remove = lines.len() - LOG_TAIL_MAX_LINES;
        lines.drain(..remove);
        truncated = true;
    }
    let line_count = lines.len();

    Ok(ServiceLogTail {
        kind,
        content: lines.join("\n"),
        source_bytes,
        line_count,
        truncated,
        exists: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_harness_profile_names() {
        for profile in ["local-agent-stack", "team_1", "profile.v2"] {
            validate_profile_name(profile).unwrap();
        }
    }

    #[test]
    fn rejects_harness_profile_path_traversal() {
        for profile in ["", ".", "..", "../web", "folder/profile", "folder\\profile"] {
            assert!(validate_profile_name(profile).is_err());
        }
    }

    #[test]
    fn builds_loopback_profile_launch_arguments() {
        let mut config = StackConfig::default();
        config.harness.url = "http://127.0.0.1:3456".into();
        let args = harness_profile_args(&config).unwrap();
        assert_eq!(
            args,
            [
                "--profile",
                "local-agent-stack",
                "--host",
                "127.0.0.1",
                "--port",
                "3456",
                "--no-open"
            ]
        );
    }

    #[test]
    fn prefixes_managed_harness_entrypoint_without_shell_composition() {
        let config = StackConfig {
            managed_harness_entrypoint: Some("C:\\App Data\\Harness\\lib\\bin.js".into()),
            ..StackConfig::default()
        };
        let args = harness_profile_args(&config).unwrap();
        assert_eq!(args.first(), config.managed_harness_entrypoint.as_ref());
        assert_eq!(args[1], "--profile");
        assert_eq!(args[2], "local-agent-stack");
    }

    #[test]
    fn plans_stack_start_in_dependency_order_and_skips_online_services() {
        assert_eq!(
            stack_start_plan(false, false),
            [ServiceKind::Ollama, ServiceKind::Harness]
        );
        assert_eq!(stack_start_plan(false, true), [ServiceKind::Ollama]);
        assert_eq!(stack_start_plan(true, false), [ServiceKind::Harness]);
        assert!(stack_start_plan(true, true).is_empty());
    }

    #[test]
    fn plans_stack_stop_in_reverse_order_and_ignores_external_services() {
        assert_eq!(
            stack_stop_plan(true, true),
            [ServiceKind::Harness, ServiceKind::Ollama]
        );
        assert_eq!(stack_stop_plan(true, false), [ServiceKind::Ollama]);
        assert_eq!(stack_stop_plan(false, true), [ServiceKind::Harness]);
        assert!(stack_stop_plan(false, false).is_empty());
    }

    #[tokio::test]
    async fn reads_only_the_bounded_tail_of_service_logs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ollama.log");
        let content = (0..300)
            .map(|index| format!("line-{index:03}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, &content).await.unwrap();

        let tail = read_service_log_tail(&path, ServiceKind::Ollama)
            .await
            .unwrap();
        assert!(tail.exists);
        assert!(tail.truncated);
        assert_eq!(tail.line_count, LOG_TAIL_MAX_LINES);
        assert!(tail.content.starts_with("line-050"));
        assert!(tail.content.ends_with("line-299"));
        assert!(!tail.content.contains("line-049"));
    }

    #[tokio::test]
    async fn reports_missing_service_logs_without_creating_content() {
        let directory = tempfile::tempdir().unwrap();
        let tail =
            read_service_log_tail(&directory.path().join("harness.log"), ServiceKind::Harness)
                .await
                .unwrap();
        assert!(!tail.exists);
        assert!(!tail.truncated);
        assert_eq!(tail.line_count, 0);
        assert!(tail.content.is_empty());
    }
}
