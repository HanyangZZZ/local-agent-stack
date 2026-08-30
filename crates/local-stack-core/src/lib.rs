mod artifact;
mod compatibility;
mod config;
mod diagnostics;
mod environment;
mod error;
mod managed;
mod ollama;
mod supervisor;
mod types;

pub use artifact::{RuntimeInstallProgress, download_and_extract_verified};
pub use compatibility::{
    CompatibilityManifest, CompatibilityReport, CompatibilityState, ComponentCompatibility,
    ComponentRequirement, RuntimeArtifact, assess_versions, embedded_artifact, embedded_manifest,
};
pub use config::{ConfigStore, StackConfig};
pub use diagnostics::export_diagnostics;
pub use environment::inspect_environment;
pub use error::{Result, StackError};
pub use managed::{
    ManagedRuntimeRelease, ManagedRuntimeState, ManagedRuntimeStatus, ManagedRuntimeStore,
};
pub use ollama::OllamaClient;
pub use supervisor::StackSupervisor;
pub use types::{
    ActionResult, EnvironmentSnapshot, GpuSnapshot, InstalledModel, PullProgress, RunningModel,
    ServiceKind, ServiceSnapshot, ServiceState, StackSnapshot,
};
