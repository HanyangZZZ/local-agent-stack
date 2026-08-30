mod config;
mod error;
mod ollama;
mod supervisor;
mod types;

pub use config::{ConfigStore, StackConfig};
pub use error::{Result, StackError};
pub use ollama::OllamaClient;
pub use supervisor::StackSupervisor;
pub use types::{
    ActionResult, InstalledModel, RunningModel, ServiceKind, ServiceSnapshot, ServiceState,
    StackSnapshot,
};
