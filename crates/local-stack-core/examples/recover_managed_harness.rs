use local_stack_core::{ServiceKind, ServiceState, StackSupervisor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let supervisor = StackSupervisor::discover().await?;
    let before = supervisor.snapshot().await?;
    if before.harness.state == ServiceState::Online && !before.harness.managed {
        return Err("the running Harness is external and cannot be recovered safely".into());
    }
    supervisor.restart(ServiceKind::Harness).await?;
    let after = supervisor.snapshot().await?;
    if after.harness.state != ServiceState::Online
        || !after.harness.managed
        || after.harness.launch_url.is_none()
    {
        return Err("Harness restarted without a manageable authenticated URL".into());
    }
    println!(
        "Managed Harness recovered on PID {}; authenticated URL captured",
        after.harness.pid.unwrap_or_default()
    );
    Ok(())
}
