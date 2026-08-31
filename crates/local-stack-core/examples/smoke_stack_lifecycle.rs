use local_stack_core::{ServiceState, StackSupervisor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let supervisor = StackSupervisor::discover().await?;
    let before = supervisor.snapshot().await?;
    if before.ollama.state != ServiceState::Offline || before.ollama.managed {
        return Err("smoke test requires Ollama to be offline".into());
    }
    if before.harness.state != ServiceState::Online || before.harness.managed {
        return Err("smoke test requires an external Harness to be online".into());
    }

    let started = supervisor.start_stack().await?;
    let running_result = supervisor.snapshot().await;
    let stopped_result = supervisor.stop_managed_stack().await;
    let running = running_result?;
    if running.ollama.state != ServiceState::Online || !running.ollama.managed {
        return Err("Start stack did not bring managed Ollama online".into());
    }
    if running.harness.state != ServiceState::Online || running.harness.managed {
        return Err("Start stack changed ownership of the external Harness".into());
    }

    let stopped = stopped_result?;
    let after = supervisor.snapshot().await?;
    if after.ollama.state != ServiceState::Offline || after.ollama.managed {
        return Err("Stop managed stack did not release managed Ollama".into());
    }
    if after.harness.state != ServiceState::Online || after.harness.managed {
        return Err("Stop managed stack interrupted the external Harness".into());
    }

    println!("{}", started.message);
    println!("{}", stopped.message);
    println!("Lifecycle smoke test passed; external Harness remained online");
    Ok(())
}
