use local_stack_core::{ActionResult, ServiceKind, StackConfig, StackSnapshot, StackSupervisor};
use tauri::State;

struct AppState(StackSupervisor);

fn parse_service(service: &str) -> Result<ServiceKind, String> {
    match service {
        "ollama" => Ok(ServiceKind::Ollama),
        "harness" => Ok(ServiceKind::Harness),
        _ => Err(format!("unknown service: {service}")),
    }
}

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> Result<StackSnapshot, String> {
    state.0.snapshot().await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<StackConfig, String> {
    Ok(state.0.config().await)
}

#[tauri::command]
async fn save_config(
    config: StackConfig,
    state: State<'_, AppState>,
) -> Result<ActionResult, String> {
    state
        .0
        .save_config(config)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_service(service: &str, state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .start(parse_service(service)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_service(service: &str, state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .stop(parse_service(service)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn restart_service(
    service: &str,
    state: State<'_, AppState>,
) -> Result<ActionResult, String> {
    state
        .0
        .restart(parse_service(service)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pull_model(model: &str, state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .pull_model(model)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn unload_model(model: &str, state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .unload_model(model)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_model(model: &str, state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .delete_model(model)
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let supervisor = tauri::async_runtime::block_on(StackSupervisor::discover())
        .expect("failed to initialize the Local Agent Stack supervisor");
    tauri::Builder::default()
        .manage(AppState(supervisor))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_config,
            save_config,
            start_service,
            stop_service,
            restart_service,
            pull_model,
            unload_model,
            delete_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Local Agent Stack");
}
