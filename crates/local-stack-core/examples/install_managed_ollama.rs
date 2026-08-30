use local_stack_core::StackSupervisor;

#[tokio::main]
async fn main() {
    let result = async {
        let supervisor = StackSupervisor::discover().await?;
        supervisor
            .install_managed_ollama_with_progress(|progress| {
                let percent = progress
                    .completed
                    .saturating_mul(100)
                    .checked_div(progress.total)
                    .unwrap_or_default();
                println!("{}: {}%", progress.message, percent);
            })
            .await
    }
    .await;

    match result {
        Ok(result) => println!("{}", result.message),
        Err(error) => {
            eprintln!("failed to install managed Ollama: {error}");
            std::process::exit(1);
        }
    }
}
