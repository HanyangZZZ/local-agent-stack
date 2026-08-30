use std::{
    fs::{self as std_fs, File, OpenOptions as StdOpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt};

use crate::{Result, RuntimeArtifact, ServiceKind, StackError};

const PROGRESS_INTERVAL_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInstallProgress {
    pub kind: ServiceKind,
    pub stage: String,
    pub completed: u64,
    pub total: u64,
    pub message: String,
}

pub async fn download_and_extract_verified<F>(
    client: &reqwest::Client,
    artifact: &RuntimeArtifact,
    staging: &Path,
    mut on_progress: F,
) -> Result<PathBuf>
where
    F: FnMut(RuntimeInstallProgress) + Send,
{
    validate_artifact(artifact)?;
    let required_space = artifact
        .download_size
        .checked_add(artifact.maximum_extracted_size)
        .ok_or_else(|| StackError::Config("runtime disk-space requirement overflowed".into()))?;
    let available_space = fs2::available_space(staging)?;
    if available_space < required_space {
        return Err(StackError::Config(format!(
            "managed runtime installation requires {} free bytes, but only {available_space} are available",
            required_space
        )));
    }
    let archive = staging.join("runtime-download.zip");
    on_progress(progress(
        artifact,
        "downloading",
        0,
        "Downloading the official runtime archive",
    ));
    let response = client.get(&artifact.url).send().await?.error_for_status()?;
    if let Some(length) = response.content_length()
        && length != artifact.download_size
    {
        return Err(StackError::Config(format!(
            "runtime download size changed: manifest expects {}, server reports {length}",
            artifact.download_size
        )));
    }

    let mut destination = fs::File::create(&archive).await?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut completed = 0_u64;
    let mut last_reported = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        completed = completed
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| StackError::Config("runtime download size overflowed".into()))?;
        if completed > artifact.download_size {
            return Err(StackError::Config(
                "runtime download exceeded the manifest size".into(),
            ));
        }
        hasher.update(&chunk);
        destination.write_all(&chunk).await?;
        if completed - last_reported >= PROGRESS_INTERVAL_BYTES {
            last_reported = completed;
            on_progress(progress(
                artifact,
                "downloading",
                completed,
                "Downloading the official runtime archive",
            ));
        }
    }
    destination.flush().await?;
    drop(destination);
    if completed != artifact.download_size {
        return Err(StackError::Config(format!(
            "runtime download is incomplete: received {completed} of {} bytes",
            artifact.download_size
        )));
    }

    on_progress(progress(
        artifact,
        "verifying",
        completed,
        "Verifying SHA-256 checksum",
    ));
    let actual = format!("{:x}", hasher.finalize());
    if actual != artifact.sha256.to_ascii_lowercase() {
        return Err(StackError::Config(format!(
            "runtime checksum mismatch: expected {}, received {actual}",
            artifact.sha256
        )));
    }

    on_progress(progress(
        artifact,
        "extracting",
        completed,
        "Extracting the verified archive",
    ));
    let archive_for_task = archive.clone();
    let staging_for_task = staging.to_path_buf();
    let maximum_extracted_size = artifact.maximum_extracted_size;
    tokio::task::spawn_blocking(move || {
        extract_zip_secure(&archive_for_task, &staging_for_task, maximum_extracted_size)
    })
    .await
    .map_err(|error| StackError::Config(format!("runtime extraction task failed: {error}")))??;
    fs::remove_file(&archive).await?;
    let executable = staging.join(portable_relative_path(&artifact.executable_relative_path)?);
    if !executable.is_file() {
        return Err(StackError::Config(format!(
            "verified runtime archive did not contain {}",
            artifact.executable_relative_path
        )));
    }
    on_progress(progress(
        artifact,
        "validating",
        artifact.download_size,
        "Validating the extracted runtime",
    ));
    Ok(executable)
}

fn validate_artifact(artifact: &RuntimeArtifact) -> Result<()> {
    let url = reqwest::Url::parse(&artifact.url)
        .map_err(|error| StackError::Config(format!("runtime artifact URL is invalid: {error}")))?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return Err(StackError::Config(
            "runtime artifacts must use an official github.com HTTPS URL".into(),
        ));
    }
    if artifact.archive_format != "zip" {
        return Err(StackError::Config(format!(
            "unsupported runtime archive format: {}",
            artifact.archive_format
        )));
    }
    if artifact.sha256.len() != 64
        || !artifact
            .sha256
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err(StackError::Config(
            "runtime artifact SHA-256 is invalid".into(),
        ));
    }
    portable_relative_path(&artifact.executable_relative_path)?;
    Ok(())
}

fn progress(
    artifact: &RuntimeArtifact,
    stage: &str,
    completed: u64,
    message: &str,
) -> RuntimeInstallProgress {
    RuntimeInstallProgress {
        kind: artifact.kind,
        stage: stage.into(),
        completed,
        total: artifact.download_size,
        message: message.into(),
    }
}

fn portable_relative_path(value: &str) -> Result<PathBuf> {
    let path: PathBuf = value.split('/').collect();
    let valid = !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !valid {
        return Err(StackError::Config(
            "runtime artifact path must remain inside its release".into(),
        ));
    }
    Ok(path)
}

fn extract_zip_secure(
    archive_path: &Path,
    destination: &Path,
    maximum_extracted_size: u64,
) -> Result<()> {
    let archive_file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|error| StackError::Config(format!("invalid runtime ZIP archive: {error}")))?;
    let mut extracted_size = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            StackError::Config(format!("failed to read runtime ZIP entry: {error}"))
        })?;
        let enclosed = entry.enclosed_name().ok_or_else(|| {
            StackError::Config(format!(
                "runtime ZIP entry escapes staging: {}",
                entry.name()
            ))
        })?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(StackError::Config(format!(
                "runtime ZIP contains an unsupported symbolic link: {}",
                entry.name()
            )));
        }
        extracted_size = extracted_size
            .checked_add(entry.size())
            .ok_or_else(|| StackError::Config("runtime extracted size overflowed".into()))?;
        if extracted_size > maximum_extracted_size {
            return Err(StackError::Config(
                "runtime archive exceeds the maximum extracted size".into(),
            ));
        }
        let output = destination.join(enclosed);
        if entry.is_dir() {
            std_fs::create_dir_all(output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std_fs::create_dir_all(parent)?;
        }
        let mut file = StdOpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_artifact_path_escape_and_untrusted_hosts() {
        let mut artifact =
            crate::embedded_artifact(ServiceKind::Ollama, "windows", "x86_64").unwrap();
        artifact.executable_relative_path = "../ollama.exe".into();
        assert!(validate_artifact(&artifact).is_err());
        artifact.executable_relative_path = "ollama.exe".into();
        artifact.url = "https://example.com/ollama.zip".into();
        assert!(validate_artifact(&artifact).is_err());
    }

    #[test]
    fn extracts_a_small_zip_inside_staging() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("fixture.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file::<_, ()>("ollama.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"fixture").unwrap();
        writer.finish().unwrap();
        let destination = directory.path().join("output");
        std_fs::create_dir(&destination).unwrap();
        extract_zip_secure(&archive_path, &destination, 1024).unwrap();
        assert_eq!(
            std_fs::read(destination.join("ollama.exe")).unwrap(),
            b"fixture"
        );
    }
}
