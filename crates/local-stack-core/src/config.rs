use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{Result, StackError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    pub url: String,
    pub command: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackConfig {
    pub ollama: ServiceConfig,
    pub harness: ServiceConfig,
    #[serde(default)]
    pub harness_home: Option<String>,
    pub harness_profile: String,
    #[serde(default)]
    pub managed_harness_node: Option<String>,
    #[serde(default)]
    pub managed_harness_source: Option<String>,
    #[serde(default)]
    pub managed_harness_entrypoint: Option<String>,
    #[serde(default)]
    pub setup_completed: bool,
    #[serde(default)]
    pub trace: TraceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceConfig {
    pub enabled: bool,
    #[serde(default)]
    pub session_root: Option<String>,
    pub gpu_sample_interval_ms: u64,
    pub inference_slots: usize,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            session_root: None,
            gpu_sample_interval_ms: 1_000,
            inference_slots: 4,
        }
    }
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            ollama: ServiceConfig {
                url: "http://127.0.0.1:11434".into(),
                command: None,
                args: vec!["serve".into()],
            },
            harness: ServiceConfig {
                url: "http://127.0.0.1:3000".into(),
                command: None,
                args: vec![
                    "web".into(),
                    "--host".into(),
                    "127.0.0.1".into(),
                    "--port".into(),
                    "3000".into(),
                    "--no-open".into(),
                ],
            },
            harness_home: None,
            harness_profile: "local-agent-stack".into(),
            managed_harness_node: None,
            managed_harness_source: None,
            managed_harness_entrypoint: None,
            setup_completed: false,
            trace: TraceConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn discover() -> Result<Self> {
        let dirs =
            ProjectDirs::from("dev", "localagentstack", "Local Agent Stack").ok_or_else(|| {
                StackError::Config("the operating-system config directory is unavailable".into())
            })?;
        Ok(Self {
            path: dirs.config_dir().join("stack.json"),
        })
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn load(&self) -> Result<StackConfig> {
        if !self.path.exists() {
            let config = StackConfig::default();
            self.save(&config).await?;
            return Ok(config);
        }

        let raw = fs::read_to_string(&self.path).await?;
        serde_json::from_str(&raw).map_err(|error| StackError::Config(error.to_string()))
    }

    pub async fn save(&self, config: &StackConfig) -> Result<()> {
        validate(config)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| StackError::Config("configuration path has no parent".into()))?;
        fs::create_dir_all(parent).await?;

        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(config)?;
        fs::write(&temporary, bytes).await?;

        if self.path.exists() {
            let backup = self.path.with_extension("json.bak");
            fs::copy(&self.path, &backup).await?;
            fs::remove_file(&self.path).await?;
            if let Err(error) = fs::rename(&temporary, &self.path).await {
                let _ = fs::copy(&backup, &self.path).await;
                return Err(error.into());
            }
        } else {
            fs::rename(&temporary, &self.path).await?;
        }
        Ok(())
    }
}

fn validate(config: &StackConfig) -> Result<()> {
    for (name, value) in [
        ("Ollama", &config.ollama.url),
        ("Harness", &config.harness.url),
    ] {
        let parsed = reqwest::Url::parse(value)
            .map_err(|error| StackError::Config(format!("{name} URL is invalid: {error}")))?;
        let local = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
        if !local {
            return Err(StackError::Config(format!(
                "{name} URL must use a loopback address in version 0.1"
            )));
        }
    }
    if config.harness_profile.trim().is_empty() {
        return Err(StackError::Config("Harness profile cannot be empty".into()));
    }
    if !(250..=60_000).contains(&config.trace.gpu_sample_interval_ms) {
        return Err(StackError::Config(
            "Trace GPU sampling interval must be between 250 and 60000 milliseconds".into(),
        ));
    }
    if !(1..=16).contains(&config.trace.inference_slots) {
        return Err(StackError::Config(
            "Trace inference slots must be between 1 and 16".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn round_trips_and_backs_up_configuration() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("stack.json");
        let store = ConfigStore::at(&path);
        let mut config = StackConfig::default();
        store.save(&config).await.unwrap();

        config.harness_profile = "test-profile".into();
        store.save(&config).await.unwrap();

        assert_eq!(store.load().await.unwrap().harness_profile, "test-profile");
        assert!(path.with_extension("json.bak").exists());
    }

    #[tokio::test]
    async fn rejects_non_loopback_control_targets() {
        let directory = tempdir().unwrap();
        let store = ConfigStore::at(directory.path().join("stack.json"));
        let mut config = StackConfig::default();
        config.ollama.url = "https://example.com".into();

        assert!(store.save(&config).await.is_err());
    }
}
