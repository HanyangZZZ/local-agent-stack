use local_stack_core::StackSupervisor;

#[tokio::main]
async fn main() {
    let result = async {
        let supervisor = StackSupervisor::discover().await?;
        supervisor.install_harness_companion().await
    }
    .await;

    match result {
        Ok(result) => println!("{}", result.message),
        Err(error) => {
            eprintln!("failed to install the Harness companion: {error}");
            std::process::exit(1);
        }
    }
}
