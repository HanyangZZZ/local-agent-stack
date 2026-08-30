use std::{process::Stdio, time::Duration};

use local_stack_core::StackSupervisor;
use tokio::{process::Command, time::sleep};

#[tokio::main]
async fn main() {
    let result = async {
        let supervisor = StackSupervisor::discover().await?;
        let config = supervisor.config().await;
        let node = config.harness.command.ok_or_else(|| {
            local_stack_core::StackError::Config("Harness command is not configured".into())
        })?;
        let entrypoint = config.managed_harness_entrypoint.ok_or_else(|| {
            local_stack_core::StackError::Config("managed Harness is not active".into())
        })?;
        let mut command = Command::new(node);
        command
            .args([
                entrypoint.as_str(),
                "--profile",
                config.harness_profile.as_str(),
                "--host",
                "127.0.0.1",
                "--port",
                "3011",
                "--no-open",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(home) = config.harness_home {
            command.env("DSH_HOME", home);
        }
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn()?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()?;
        let mut healthy = false;
        for _ in 0..30 {
            sleep(Duration::from_millis(500)).await;
            if client
                .get("http://127.0.0.1:3011/")
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                healthy = true;
                break;
            }
        }
        child.kill().await?;
        let _ = child.wait().await;
        if !healthy {
            return Err(local_stack_core::StackError::Config(
                "managed Harness did not become healthy on port 3011".into(),
            ));
        }
        Ok::<_, local_stack_core::StackError>(())
    }
    .await;

    match result {
        Ok(()) => println!("Managed Harness became healthy on port 3011 and stopped cleanly"),
        Err(error) => {
            eprintln!("managed Harness smoke test failed: {error}");
            std::process::exit(1);
        }
    }
}
