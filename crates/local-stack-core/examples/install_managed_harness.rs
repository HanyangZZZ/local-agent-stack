use local_stack_core::StackSupervisor;

#[tokio::main]
async fn main() {
    let result = async {
        let supervisor = StackSupervisor::discover().await?;
        supervisor.install_managed_harness().await
    }
    .await;

    match result {
        Ok(result) => println!("{}", result.message),
        Err(error) => {
            eprintln!("failed to install managed Harness: {error}");
            std::process::exit(1);
        }
    }
}
