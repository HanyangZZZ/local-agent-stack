use local_stack_core::StackSupervisor;

#[tokio::main]
async fn main() {
    let result = async {
        let supervisor = StackSupervisor::discover().await?;
        supervisor.prepare_harness_profile().await
    }
    .await;

    match result {
        Ok(result) => println!("{}", result.message),
        Err(error) => {
            eprintln!("failed to prepare the Harness profile: {error}");
            std::process::exit(1);
        }
    }
}
