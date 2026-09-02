use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};
use tokio::{fs, sync::Mutex};

use crate::{Result, ServiceKind, StackError};

const REGISTRY_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManagedProcessRecord {
    pub kind: ServiceKind,
    pub pid: u32,
    pub started_at_unix: u64,
    pub executable: String,
    pub args: Vec<String>,
    pub authenticated_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryState {
    version: u32,
    processes: HashMap<ServiceKind, ManagedProcessRecord>,
}

impl Default for RegistryState {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            processes: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessRegistry {
    path: PathBuf,
    state: Arc<Mutex<RegistryState>>,
}

impl ProcessRegistry {
    pub async fn at(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let state = load_state(&path).await?;
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(state)),
        })
    }

    pub async fn records(&self) -> HashMap<ServiceKind, ManagedProcessRecord> {
        self.state.lock().await.processes.clone()
    }

    pub async fn upsert(&self, record: ManagedProcessRecord) -> Result<()> {
        let mut state = self.state.lock().await;
        state.processes.insert(record.kind, record);
        save_state(&self.path, &state).await
    }

    pub async fn remove(&self, kind: ServiceKind) -> Result<Option<ManagedProcessRecord>> {
        let mut state = self.state.lock().await;
        let removed = state.processes.remove(&kind);
        if removed.is_some() {
            save_state(&self.path, &state).await?;
        }
        Ok(removed)
    }

    pub async fn set_authenticated_url(
        &self,
        kind: ServiceKind,
        authenticated_url: Option<String>,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let record = state.processes.get_mut(&kind).ok_or_else(|| {
            StackError::Config(format!("{kind} process record disappeared during startup"))
        })?;
        record.authenticated_url = authenticated_url;
        save_state(&self.path, &state).await
    }
}

async fn load_state(path: &Path) -> Result<RegistryState> {
    let backup = path.with_extension("json.bak");
    let bytes = if path.is_file() {
        fs::read(path).await?
    } else if backup.is_file() {
        fs::read(&backup).await?
    } else {
        return Ok(RegistryState::default());
    };
    let state: RegistryState = match serde_json::from_slice(&bytes) {
        Ok(state) => state,
        Err(primary_error) if path.is_file() && backup.is_file() => {
            let backup_bytes = fs::read(&backup).await?;
            serde_json::from_slice(&backup_bytes).map_err(|_| primary_error)?
        }
        Err(error) => return Err(error.into()),
    };
    if state.version != REGISTRY_VERSION {
        return Err(StackError::Config(format!(
            "unsupported managed-process registry version {}",
            state.version
        )));
    }
    Ok(state)
}

async fn save_state(path: &Path, state: &RegistryState) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| StackError::Config("managed-process registry path has no parent".into()))?;
    fs::create_dir_all(parent).await?;
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    fs::write(&temporary, serde_json::to_vec_pretty(state)?).await?;
    if path.is_file() {
        fs::copy(path, &backup).await?;
        fs::remove_file(path).await?;
    }
    if let Err(error) = fs::rename(&temporary, path).await {
        if backup.is_file() {
            let _ = fs::copy(&backup, path).await;
        }
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn inspect_process(system: &System, pid: u32) -> Option<ManagedProcessRecord> {
    let process = system.process(Pid::from_u32(pid))?;
    let executable = process.exe()?.to_string_lossy().into_owned();
    Some(ManagedProcessRecord {
        kind: ServiceKind::Harness,
        pid,
        started_at_unix: process.start_time(),
        executable,
        args: process
            .cmd()
            .iter()
            .skip(1)
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        authenticated_url: None,
    })
}

pub(crate) fn record_matches_process(record: &ManagedProcessRecord, system: &System) -> bool {
    let Some(process) = system.process(Pid::from_u32(record.pid)) else {
        return false;
    };
    process.start_time() == record.started_at_unix
        && process
            .exe()
            .is_some_and(|executable| paths_match(executable, Path::new(&record.executable)))
        && command_line_matches(process.cmd(), &record.args)
}

pub(crate) fn find_matching_processes(
    system: &System,
    executable: &Path,
    args: &[String],
) -> Vec<ManagedProcessRecord> {
    system
        .processes()
        .values()
        .filter(|process| {
            process
                .exe()
                .is_some_and(|candidate| paths_match(candidate, executable))
                && command_line_matches(process.cmd(), args)
        })
        .filter_map(|process| {
            let pid = process.pid().as_u32();
            inspect_process(system, pid)
        })
        .collect()
}

fn command_line_matches(command: &[std::ffi::OsString], expected_args: &[String]) -> bool {
    if command.is_empty() {
        return true;
    }
    if command.len() < expected_args.len() + 1 {
        return false;
    }
    command[command.len() - expected_args.len()..]
        .iter()
        .zip(expected_args)
        .all(|(actual, expected)| os_values_match(actual, OsStr::new(expected)))
}

fn paths_match(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn os_values_match(left: &OsStr, right: &OsStr) -> bool {
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn record() -> ManagedProcessRecord {
        ManagedProcessRecord {
            kind: ServiceKind::Harness,
            pid: 42,
            started_at_unix: 100,
            executable: "C:\\managed\\node.exe".into(),
            args: vec!["dsh.js".into(), "--port".into(), "3000".into()],
            authenticated_url: Some("http://127.0.0.1:3000/?token=secret".into()),
        }
    }

    #[tokio::test]
    async fn persists_process_identity_and_authenticated_url() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("processes.json");
        let registry = ProcessRegistry::at(&path).await.unwrap();
        registry.upsert(record()).await.unwrap();

        let reloaded = ProcessRegistry::at(&path).await.unwrap();
        assert_eq!(
            reloaded.records().await.get(&ServiceKind::Harness),
            Some(&record())
        );
    }

    #[tokio::test]
    async fn removes_only_the_selected_process_record() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("processes.json");
        let registry = ProcessRegistry::at(&path).await.unwrap();
        registry.upsert(record()).await.unwrap();
        assert!(
            registry
                .remove(ServiceKind::Harness)
                .await
                .unwrap()
                .is_some()
        );
        assert!(registry.records().await.is_empty());
    }

    #[tokio::test]
    async fn recovers_the_last_valid_registry_when_the_primary_is_corrupt() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("processes.json");
        let registry = ProcessRegistry::at(&path).await.unwrap();
        registry.upsert(record()).await.unwrap();
        let mut updated = record();
        updated.pid = 43;
        registry.upsert(updated).await.unwrap();
        fs::write(&path, b"not json").await.unwrap();

        let recovered = ProcessRegistry::at(&path).await.unwrap();
        assert_eq!(
            recovered
                .records()
                .await
                .get(&ServiceKind::Harness)
                .map(|record| record.pid),
            Some(42)
        );
    }

    #[test]
    fn matches_the_expected_command_suffix() {
        let command = ["node.exe", "dsh.js", "--port", "3000"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert!(command_line_matches(
            &command,
            &["dsh.js".into(), "--port".into(), "3000".into()]
        ));
        assert!(!command_line_matches(
            &command,
            &["dsh.js".into(), "--port".into(), "3001".into()]
        ));
    }
}
