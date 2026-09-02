use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::UserDirs;
use serde_json::{Value, json};
use tokio::fs;

use crate::{Result, StackConfig, StackError, StackSnapshot};

pub async fn export_diagnostics(
    mut snapshot: StackSnapshot,
    config: &StackConfig,
    fallback_directory: &Path,
) -> Result<PathBuf> {
    redact_snapshot(&mut snapshot);
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| StackError::Config(error.to_string()))?;
    let generated_at_unix = generated_at.as_secs();
    let report = json!({
        "schemaVersion": 1,
        "generatedAtUnix": generated_at_unix,
        "applicationVersion": env!("CARGO_PKG_VERSION"),
        "privacy": {
            "fullPathsIncluded": false,
            "logsIncluded": false,
            "credentialsIncluded": false,
            "promptsIncluded": false
        },
        "configuration": redacted_config(config),
        "snapshot": snapshot
    });

    let destination_directory = UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| fallback_directory.join("diagnostics"));
    fs::create_dir_all(&destination_directory).await?;
    let destination = destination_directory.join(format!(
        "local-agent-stack-diagnostics-{}.json",
        generated_at.as_millis()
    ));
    let temporary = destination.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(&report)?).await?;
    fs::rename(&temporary, &destination).await?;
    Ok(destination)
}

fn redacted_config(config: &StackConfig) -> Value {
    json!({
        "ollama": {
            "url": config.ollama.url,
            "executable": executable_name(config.ollama.command.as_deref()),
            "argumentCount": config.ollama.args.len()
        },
        "harness": {
            "url": config.harness.url,
            "executable": executable_name(config.harness.command.as_deref()),
            "argumentCount": config.harness.args.len(),
            "profile": config.harness_profile,
            "managedNodeExecutable": executable_name(config.managed_harness_node.as_deref()),
            "managedSourceConfigured": config.managed_harness_source.is_some(),
            "managedEntrypointConfigured": config.managed_harness_entrypoint.is_some()
        },
        "setupCompleted": config.setup_completed
    })
}

fn redact_snapshot(snapshot: &mut StackSnapshot) {
    snapshot.config_path = "[redacted]".into();
    snapshot.ollama.message = None;
    snapshot.harness.message = None;
    snapshot.ollama.launch_url = None;
    snapshot.harness.launch_url = None;
    snapshot.environment.node_path = executable_name(snapshot.environment.node_path.as_deref());
    snapshot.environment.git_path = executable_name(snapshot.environment.git_path.as_deref());
    snapshot.environment.ollama_path = executable_name(snapshot.environment.ollama_path.as_deref());
    snapshot.environment.harness_path =
        executable_name(snapshot.environment.harness_path.as_deref());
}

fn executable_name(value: Option<&str>) -> Option<String> {
    value.map(|value| {
        value
            .rsplit(['/', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or("configured")
            .to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_paths_and_arguments_from_configuration() {
        let mut config = StackConfig::default();
        config.harness.command = Some("C:\\Users\\person\\private\\dsh.cmd".into());
        config.harness.args = vec!["--token".into(), "very-secret".into()];
        config.harness_home = Some("C:\\Users\\person\\.dsh".into());
        let value = redacted_config(&config).to_string();
        assert!(value.contains("dsh.cmd"));
        assert!(!value.contains("person"));
        assert!(!value.contains("very-secret"));
        assert!(!value.contains("harness_home"));
    }
}
