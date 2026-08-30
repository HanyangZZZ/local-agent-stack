use local_stack_core::StackSupervisor;

#[tokio::main]
async fn main() {
    let result = async {
        let supervisor = StackSupervisor::discover().await?;
        supervisor.export_diagnostic_report().await
    }
    .await;

    match result {
        Ok(result) => println!("{}", result.message),
        Err(error) => {
            eprintln!("failed to export diagnostics: {error}");
            std::process::exit(1);
        }
    }
}
