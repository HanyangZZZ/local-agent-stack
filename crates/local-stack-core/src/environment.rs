use std::path::PathBuf;

use tokio::process::Command;

use crate::{EnvironmentSnapshot, GpuSnapshot};

pub async fn inspect_environment() -> EnvironmentSnapshot {
    let gpus = match which::which("nvidia-smi") {
        Ok(executable) => inspect_nvidia(executable).await,
        Err(_) => Vec::new(),
    };

    EnvironmentSnapshot {
        operating_system: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        node_path: command_path("node"),
        git_path: command_path("git"),
        ollama_path: command_path("ollama").or_else(default_ollama_path),
        harness_path: command_path("dsh").or_else(|| command_path("npx")),
        gpus,
    }
}

fn command_path(name: &str) -> Option<String> {
    which::which(name)
        .ok()
        .map(|path| path.display().to_string())
}

#[cfg(windows)]
fn default_ollama_path() -> Option<String> {
    let path = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Programs/Ollama/ollama.exe");
    path.is_file().then(|| path.display().to_string())
}

#[cfg(not(windows))]
fn default_ollama_path() -> Option<String> {
    None
}

async fn inspect_nvidia(executable: PathBuf) -> Vec<GpuSnapshot> {
    let output = Command::new(executable)
        .args([
            "--query-gpu=name,driver_version,memory.total,memory.used,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() => {
            parse_nvidia_csv(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

fn parse_nvidia_csv(value: &str) -> Vec<GpuSnapshot> {
    value
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            if fields.len() != 5 {
                return None;
            }
            Some(GpuSnapshot {
                name: fields[0].into(),
                driver_version: fields[1].into(),
                memory_total_mib: fields[2].parse().ok()?,
                memory_used_mib: fields[3].parse().ok()?,
                memory_free_mib: fields[4].parse().ok()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_nvidia_gpus() {
        let result = parse_nvidia_csv(
            "NVIDIA GeForce RTX 5090, 581.80, 32607, 12288, 20319\nNVIDIA RTX 4000, 581.80, 16376, 16, 16360\n",
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "NVIDIA GeForce RTX 5090");
        assert_eq!(result[0].memory_total_mib, 32607);
        assert_eq!(result[1].memory_free_mib, 16360);
    }

    #[test]
    fn ignores_malformed_nvidia_rows() {
        let result = parse_nvidia_csv("not,a,complete,row\n");
        assert!(result.is_empty());
    }
}
