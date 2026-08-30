use local_stack_core::{ServiceKind, ServiceState, StackSupervisor};

#[tokio::main]
async fn main() {
    let result = async {
        let supervisor = StackSupervisor::discover().await?;
        let started = supervisor.start(ServiceKind::Ollama).await?;
        println!("{}", started.message);

        let snapshot_result = supervisor.snapshot().await;
        let stop_result = supervisor.stop(ServiceKind::Ollama).await;
        let snapshot = snapshot_result?;
        stop_result?;

        if snapshot.ollama.state != ServiceState::Online {
            return Err(local_stack_core::StackError::Config(
                "managed Ollama did not become healthy".into(),
            ));
        }
        if snapshot.ollama.version.as_deref() != Some("0.33.2") {
            return Err(local_stack_core::StackError::Config(format!(
                "managed Ollama reported an unexpected version: {:?}",
                snapshot.ollama.version
            )));
        }

        Ok::<_, local_stack_core::StackError>(())
    }
    .await;

    match result {
        Ok(()) => println!("Managed Ollama 0.33.2 became healthy and stopped cleanly"),
        Err(error) => {
            eprintln!("managed Ollama smoke test failed: {error}");
            std::process::exit(1);
        }
    }
}
