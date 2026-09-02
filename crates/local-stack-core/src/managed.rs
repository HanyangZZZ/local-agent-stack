use std::{
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use directories::ProjectDirs;
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{Result, ServiceKind, StackError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeRelease {
    pub release_id: String,
    pub version: String,
    pub executable_relative_path: String,
    pub activated_at_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeState {
    pub current: Option<ManagedRuntimeRelease>,
    pub previous: Option<ManagedRuntimeRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeStatus {
    pub installed: bool,
    pub current_version: Option<String>,
    pub previous_version: Option<String>,
    pub can_rollback: bool,
}

#[derive(Debug, Clone)]
pub struct ManagedRuntimeStore {
    root: PathBuf,
}

impl ManagedRuntimeStore {
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "localagentstack", "Local Agent Stack")
            .ok_or_else(|| StackError::Config("the local data directory is unavailable".into()))?;
        Ok(Self::at(dirs.data_local_dir().join("runtimes")))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn status(&self, kind: ServiceKind) -> Result<ManagedRuntimeStatus> {
        let state = self.state(kind).await?;
        Ok(ManagedRuntimeStatus {
            installed: state.current.is_some(),
            current_version: state
                .current
                .as_ref()
                .map(|release| release.version.clone()),
            previous_version: state
                .previous
                .as_ref()
                .map(|release| release.version.clone()),
            can_rollback: state.previous.is_some(),
        })
    }

    pub async fn owns_executable(&self, kind: ServiceKind, executable: &Path) -> bool {
        let releases = self.component_dir(kind).join("releases");
        if !releases.is_dir() || !executable.is_file() {
            return false;
        }
        let Ok(releases) = fs::canonicalize(releases).await else {
            return false;
        };
        let Ok(executable) = fs::canonicalize(executable).await else {
            return false;
        };
        executable.starts_with(releases)
    }

    pub async fn create_staging_directory(
        &self,
        kind: ServiceKind,
        version: &str,
    ) -> Result<PathBuf> {
        validate_version(version)?;
        let nonce = now_millis()?;
        let staging = self
            .component_dir(kind)
            .join("staging")
            .join(format!("{version}-{nonce}"));
        fs::create_dir_all(&staging).await?;
        Ok(staging)
    }

    pub async fn abandon_staging(&self, kind: ServiceKind, staging: &Path) -> Result<()> {
        self.validate_staging_path(kind, staging).await?;
        if staging.is_dir() {
            fs::remove_dir_all(staging).await?;
        }
        Ok(())
    }

    pub async fn cleanup_staging_older_than(
        &self,
        kind: ServiceKind,
        maximum_age: Duration,
    ) -> Result<usize> {
        let root = self.component_dir(kind).join("staging");
        fs::create_dir_all(&root).await?;
        let now = SystemTime::now();
        let mut removed = 0;
        let mut entries = fs::read_dir(&root).await?;
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            let modified = entry.metadata().await?.modified()?;
            let age = now.duration_since(modified).unwrap_or_default();
            if age < maximum_age {
                continue;
            }
            self.validate_staging_path(kind, &entry.path()).await?;
            fs::remove_dir_all(entry.path()).await?;
            removed += 1;
        }
        Ok(removed)
    }

    pub async fn activate_staging(
        &self,
        kind: ServiceKind,
        version: &str,
        staging: &Path,
        executable_relative_path: &Path,
    ) -> Result<PathBuf> {
        validate_version(version)?;
        validate_relative_executable(executable_relative_path)?;
        self.validate_staging_path(kind, staging).await?;
        if !staging.join(executable_relative_path).is_file() {
            return Err(StackError::Config(
                "managed runtime validation did not produce the expected executable".into(),
            ));
        }

        let release_id = format!("{version}-{}", now_millis()?);
        let releases = self.component_dir(kind).join("releases");
        fs::create_dir_all(&releases).await?;
        let destination = releases.join(&release_id);
        let mut state = self.state(kind).await?;
        state.previous = state.current.take();
        state.current = Some(ManagedRuntimeRelease {
            release_id,
            version: version.into(),
            executable_relative_path: path_to_portable_string(executable_relative_path),
            activated_at_unix: now_seconds()?,
        });
        fs::rename(staging, &destination).await?;
        if let Err(error) = self.save_state(kind, &state).await {
            let _ = fs::rename(&destination, staging).await;
            return Err(error);
        }
        self.current_executable(kind, &state).await
    }

    pub async fn rollback(&self, kind: ServiceKind) -> Result<PathBuf> {
        let mut state = self.state(kind).await?;
        let previous = state.previous.take().ok_or_else(|| {
            StackError::Config(format!("no previous {kind} release is available"))
        })?;
        let current = state.current.replace(previous);
        state.previous = current;
        let executable = self.current_executable(kind, &state).await?;
        self.save_state(kind, &state).await?;
        Ok(executable)
    }

    async fn state(&self, kind: ServiceKind) -> Result<ManagedRuntimeState> {
        let path = self.component_dir(kind).join("state.json");
        if !path.is_file() {
            return Ok(ManagedRuntimeState::default());
        }
        let bytes = fs::read(path).await?;
        serde_json::from_slice(&bytes).map_err(StackError::from)
    }

    async fn save_state(&self, kind: ServiceKind, state: &ManagedRuntimeState) -> Result<()> {
        let directory = self.component_dir(kind);
        fs::create_dir_all(&directory).await?;
        let path = directory.join("state.json");
        let temporary = directory.join("state.json.tmp");
        let backup = directory.join("state.json.bak");
        fs::write(&temporary, serde_json::to_vec_pretty(state)?).await?;
        if path.is_file() {
            fs::copy(&path, &backup).await?;
            fs::remove_file(&path).await?;
        }
        if let Err(error) = fs::rename(temporary, &path).await {
            if backup.is_file() {
                let _ = fs::copy(backup, &path).await;
            }
            return Err(error.into());
        }
        Ok(())
    }

    async fn current_executable(
        &self,
        kind: ServiceKind,
        state: &ManagedRuntimeState,
    ) -> Result<PathBuf> {
        let release = state
            .current
            .as_ref()
            .ok_or_else(|| StackError::Config(format!("no managed {kind} release is active")))?;
        if !is_single_path_component(&release.release_id) {
            return Err(StackError::Config(
                "managed runtime state contains an invalid release identifier".into(),
            ));
        }
        let relative = portable_string_to_path(&release.executable_relative_path);
        validate_relative_executable(&relative)?;
        let executable = self
            .component_dir(kind)
            .join("releases")
            .join(&release.release_id)
            .join(relative);
        if !executable.is_file() {
            return Err(StackError::Config(format!(
                "managed {kind} executable is missing from release {}",
                release.release_id
            )));
        }
        Ok(executable)
    }

    async fn validate_staging_path(&self, kind: ServiceKind, staging: &Path) -> Result<()> {
        let staging_root = self.component_dir(kind).join("staging");
        fs::create_dir_all(&staging_root).await?;
        let canonical_root = fs::canonicalize(staging_root).await?;
        let canonical_staging = fs::canonicalize(staging).await?;
        if canonical_staging.parent() != Some(canonical_root.as_path()) {
            return Err(StackError::Config(
                "managed runtime staging directory is outside app-owned storage".into(),
            ));
        }
        Ok(())
    }

    fn component_dir(&self, kind: ServiceKind) -> PathBuf {
        self.root.join(kind.to_string().to_lowercase())
    }
}

fn validate_version(version: &str) -> Result<()> {
    Version::parse(version)
        .map(|_| ())
        .map_err(|error| StackError::Config(format!("invalid managed runtime version: {error}")))
}

fn validate_relative_executable(path: &Path) -> Result<()> {
    let valid = !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if valid {
        Ok(())
    } else {
        Err(StackError::Config(
            "managed runtime executable path must stay inside its release".into(),
        ))
    }
}

fn is_single_path_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn path_to_portable_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn portable_string_to_path(value: &str) -> PathBuf {
    value.split('/').collect()
}

fn now_seconds() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| StackError::Config(error.to_string()))?
        .as_secs())
}

fn now_millis() -> Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| StackError::Config(error.to_string()))?
        .as_millis())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn activates_side_by_side_releases_and_rolls_back() {
        let directory = tempdir().unwrap();
        let store = ManagedRuntimeStore::at(directory.path().join("runtimes"));
        let relative = Path::new("bin/dsh");

        let first = store
            .create_staging_directory(ServiceKind::Harness, "0.1.0")
            .await
            .unwrap();
        fs::create_dir_all(first.join("bin")).await.unwrap();
        fs::write(first.join(relative), b"first").await.unwrap();
        let first_executable = store
            .activate_staging(ServiceKind::Harness, "0.1.0", &first, relative)
            .await
            .unwrap();

        let second = store
            .create_staging_directory(ServiceKind::Harness, "0.2.0")
            .await
            .unwrap();
        fs::create_dir_all(second.join("bin")).await.unwrap();
        fs::write(second.join(relative), b"second").await.unwrap();
        let second_executable = store
            .activate_staging(ServiceKind::Harness, "0.2.0", &second, relative)
            .await
            .unwrap();

        let status = store.status(ServiceKind::Harness).await.unwrap();
        assert_eq!(status.current_version.as_deref(), Some("0.2.0"));
        assert_eq!(status.previous_version.as_deref(), Some("0.1.0"));
        assert!(status.can_rollback);
        assert_ne!(first_executable, second_executable);

        let rolled_back = store.rollback(ServiceKind::Harness).await.unwrap();
        assert_eq!(fs::read(rolled_back).await.unwrap(), b"first");
        assert!(second_executable.is_file());
    }

    #[tokio::test]
    async fn rejects_staging_and_executable_path_escape() {
        let directory = tempdir().unwrap();
        let store = ManagedRuntimeStore::at(directory.path().join("runtimes"));
        let outside = directory.path().join("outside");
        fs::create_dir_all(&outside).await.unwrap();
        assert!(
            store
                .activate_staging(ServiceKind::Harness, "0.1.0", &outside, Path::new("../dsh"))
                .await
                .is_err()
        );
    }
}
