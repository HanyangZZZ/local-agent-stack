use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{InstalledModel, Result, RunningModel, StackError};

#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    client: Client,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let client = Client::builder().timeout(Duration::from_secs(8)).build()?;
        Ok(Self { base_url, client })
    }

    pub async fn version(&self) -> Result<String> {
        let response = self
            .client
            .get(format!("{}/api/version", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json::<VersionResponse>().await?.version)
    }

    pub async fn installed_models(&self) -> Result<Vec<InstalledModel>> {
        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        let mut models = response.json::<TagsResponse>().await?.models;
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(models
            .into_iter()
            .map(|model| InstalledModel {
                name: model.name,
                size: model.size,
                parameter_size: model
                    .details
                    .as_ref()
                    .and_then(|value| value.parameter_size.clone()),
                quantization_level: model.details.and_then(|value| value.quantization_level),
            })
            .collect())
    }

    pub async fn running_models(&self) -> Result<Vec<RunningModel>> {
        let response = self
            .client
            .get(format!("{}/api/ps", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        let mut models = response.json::<PsResponse>().await?.models;
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(models)
    }

    pub async fn pull_model(&self, model: &str) -> Result<()> {
        validate_model_name(model)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(60 * 60))
            .build()?;
        client
            .post(format!("{}/api/pull", self.base_url))
            .json(&PullRequest {
                model,
                stream: false,
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn unload_model(&self, model: &str) -> Result<()> {
        validate_model_name(model)?;
        self.client
            .post(format!("{}/api/generate", self.base_url))
            .json(&UnloadRequest {
                model,
                keep_alive: 0,
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn delete_model(&self, model: &str) -> Result<()> {
        validate_model_name(model)?;
        let response = self
            .client
            .delete(format!("{}/api/delete", self.base_url))
            .json(&DeleteRequest { model })
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(StackError::Config(format!(
                "model {model} is not installed"
            )));
        }
        response.error_for_status()?;
        Ok(())
    }
}

fn validate_model_name(model: &str) -> Result<()> {
    if model.trim().is_empty() || model.len() > 240 || model.chars().any(char::is_control) {
        return Err(StackError::Config("invalid model name".into()));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    version: String,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    name: String,
    #[serde(default)]
    size: u64,
    details: Option<ModelDetails>,
}

#[derive(Debug, Deserialize)]
struct ModelDetails {
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PsResponse {
    #[serde(default)]
    models: Vec<RunningModel>,
}

#[derive(Debug, Serialize)]
struct PullRequest<'a> {
    model: &'a str,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct UnloadRequest<'a> {
    model: &'a str,
    keep_alive: u8,
}

#[derive(Debug, Serialize)]
struct DeleteRequest<'a> {
    model: &'a str,
}
