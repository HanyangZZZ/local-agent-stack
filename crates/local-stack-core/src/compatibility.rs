use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{Result, ServiceKind, StackError};

const EMBEDDED_MANIFEST: &str = include_str!("../../../manifests/compatibility.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityManifest {
    pub schema_version: u32,
    pub updated_at: String,
    pub components: Vec<ComponentRequirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentRequirement {
    pub kind: ServiceKind,
    pub display_name: String,
    pub minimum_version: Version,
    pub maximum_version_exclusive: Version,
    pub recommended_version: Version,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompatibilityState {
    Compatible,
    Outdated,
    Untested,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentCompatibility {
    pub kind: ServiceKind,
    pub display_name: String,
    pub detected_version: Option<String>,
    pub recommended_version: String,
    pub state: CompatibilityState,
    pub message: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityReport {
    pub manifest_updated_at: String,
    pub components: Vec<ComponentCompatibility>,
}

pub fn embedded_manifest() -> Result<CompatibilityManifest> {
    serde_json::from_str(EMBEDDED_MANIFEST).map_err(|error| {
        StackError::Config(format!("invalid embedded compatibility manifest: {error}"))
    })
}

pub fn assess_versions(
    ollama_version: Option<&str>,
    harness_version: Option<&str>,
) -> Result<CompatibilityReport> {
    let manifest = embedded_manifest()?;
    let components = manifest
        .components
        .iter()
        .map(|requirement| {
            let detected = match requirement.kind {
                ServiceKind::Ollama => ollama_version,
                ServiceKind::Harness => harness_version,
            };
            assess_component(requirement, detected)
        })
        .collect();
    Ok(CompatibilityReport {
        manifest_updated_at: manifest.updated_at,
        components,
    })
}

fn assess_component(
    requirement: &ComponentRequirement,
    detected_version: Option<&str>,
) -> ComponentCompatibility {
    let parsed = detected_version.and_then(parse_version);
    let (state, message) = match parsed {
        Some(version) if version < requirement.minimum_version => (
            CompatibilityState::Outdated,
            format!(
                "Upgrade required; tested versions start at {}",
                requirement.minimum_version
            ),
        ),
        Some(version) if version >= requirement.maximum_version_exclusive => (
            CompatibilityState::Untested,
            format!(
                "Newer than the tested range ending before {}",
                requirement.maximum_version_exclusive
            ),
        ),
        Some(_) => (
            CompatibilityState::Compatible,
            "Within the tested compatibility range".into(),
        ),
        None => (
            CompatibilityState::Unknown,
            "Version unavailable; runtime may be offline or not installed".into(),
        ),
    };

    ComponentCompatibility {
        kind: requirement.kind,
        display_name: requirement.display_name.clone(),
        detected_version: detected_version.map(str::to_owned),
        recommended_version: requirement.recommended_version.to_string(),
        state,
        message,
        source: requirement.source.clone(),
    }
}

fn parse_version(value: &str) -> Option<Version> {
    value
        .split_whitespace()
        .find_map(|part| Version::parse(part.trim_start_matches('v')).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_versions_inside_the_tested_range() {
        let report = assess_versions(Some("v0.33.2"), Some("0.1.1-rc.2")).unwrap();
        assert!(
            report
                .components
                .iter()
                .all(|component| component.state == CompatibilityState::Compatible)
        );
    }

    #[test]
    fn distinguishes_old_new_and_unknown_versions() {
        let report = assess_versions(Some("0.11.0"), Some("dsh 0.2.0")).unwrap();
        assert_eq!(report.components[0].state, CompatibilityState::Outdated);
        assert_eq!(report.components[1].state, CompatibilityState::Untested);

        let unknown = assess_versions(None, Some("not-semver")).unwrap();
        assert!(
            unknown
                .components
                .iter()
                .all(|component| component.state == CompatibilityState::Unknown)
        );
    }
}
