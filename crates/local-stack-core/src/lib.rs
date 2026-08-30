mod config;
mod environment;
mod error;
mod ollama;
mod supervisor;
mod types;

pub use config::{ConfigStore, StackConfig};
pub use environment::inspect_environment;
pub use error::{Result, StackError};
pub use ollama::OllamaClient;
pub use supervisor::StackSupervisor;
pub use types::{
    ActionResult, EnvironmentSnapshot, GpuSnapshot, InstalledModel, PullProgress, RunningModel,
    ServiceKind, ServiceSnapshot, ServiceState, StackSnapshot,
};
