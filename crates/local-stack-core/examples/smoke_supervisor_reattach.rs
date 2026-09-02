use local_stack_core::{ConfigStore, ServiceKind, ServiceState, StackSupervisor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let installed = StackSupervisor::discover().await?;
    let mut config = installed.config().await;
    config.ollama.url = "http://127.0.0.1:65534".into();
    config.harness.url = "http://127.0.0.1:3011".into();
    let port = config
        .harness
        .args
        .iter()
        .position(|argument| argument == "--port")
        .ok_or("configured Harness arguments do not contain --port")?;
    config.harness.args[port + 1] = "3011".into();

    let directory = tempfile::tempdir()?;
    let store = ConfigStore::at(directory.path().join("stack.json"));
    let first = StackSupervisor::new(store.clone()).await?;
    first.save_config(config).await?;
    first.start(ServiceKind::Harness).await?;
    let started = first.snapshot().await?;
    if started.harness.state != ServiceState::Online
        || !started.harness.managed
        || started.harness.launch_url.is_none()
    {
        return Err("first supervisor did not capture a manageable authenticated Harness".into());
    }
    let pid = started.harness.pid;
    drop(first);

    let second = StackSupervisor::new(store).await?;
    let recovered = second.snapshot().await?;
    if recovered.harness.state != ServiceState::Online
        || !recovered.harness.managed
        || recovered.harness.pid != pid
        || recovered.harness.launch_url.is_none()
    {
        return Err("second supervisor did not recover the persisted Harness identity".into());
    }
    second.stop(ServiceKind::Harness).await?;
    let stopped = second.snapshot().await?;
    if stopped.harness.state != ServiceState::Offline || stopped.harness.managed {
        return Err("recovered Harness did not stop cleanly".into());
    }

    println!("Supervisor restart recovery passed for Harness PID {pid:?}");
    Ok(())
}
