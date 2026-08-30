use local_stack_core::StackSupervisor;

#[tokio::main]
async fn main() {
    match StackSupervisor::discover().await {
        Ok(supervisor) => match supervisor.snapshot().await {
            Ok(snapshot) => println!(
                "{}",
                serde_json::to_string_pretty(&snapshot)
                    .expect("stack snapshot should always serialize")
            ),
            Err(error) => {
                eprintln!("failed to inspect the local stack: {error}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("failed to initialize the local stack: {error}");
            std::process::exit(1);
        }
    }
}
