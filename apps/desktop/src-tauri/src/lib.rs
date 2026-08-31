use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use local_stack_core::{
    ActionResult, RuntimeInstallProgress, ServiceKind, StackConfig, StackSnapshot, StackSupervisor,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

struct AppState(StackSupervisor);
struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateMetadata {
    version: String,
    current_version: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateProgress {
    downloaded: u64,
    total: Option<u64>,
    message: String,
}

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
async fn pull_model(
    model: &str,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ActionResult, String> {
    state
        .0
        .pull_model_with_progress(model, move |progress| {
            let _ = app.emit("ollama-pull-progress", progress);
        })
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
async fn unload_all_models(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .unload_all_models()
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

#[tauri::command]
async fn prepare_harness_profile(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .prepare_harness_profile()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn install_harness_companion(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .install_harness_companion()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn install_managed_ollama(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ActionResult, String> {
    state
        .0
        .install_managed_ollama_with_progress(move |progress: RuntimeInstallProgress| {
            let _ = app.emit("runtime-install-progress", progress);
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn rollback_managed_ollama(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .rollback_managed_ollama()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn install_managed_harness(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .install_managed_harness()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn rollback_managed_harness(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .rollback_managed_harness()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn complete_setup(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .complete_setup()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn export_diagnostics(state: State<'_, AppState>) -> Result<ActionResult, String> {
    state
        .0
        .export_diagnostic_report()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn check_for_app_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<AppUpdateMetadata>, String> {
    let update = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?;
    let metadata = update.as_ref().map(|update| AppUpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
    });
    *pending
        .0
        .lock()
        .map_err(|_| "the pending update lock is unavailable".to_string())? = update;
    Ok(metadata)
}

#[tauri::command]
async fn install_app_update(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let snapshot = state
        .0
        .snapshot()
        .await
        .map_err(|error| error.to_string())?;
    if snapshot.ollama.managed || snapshot.harness.managed {
        return Err(
            "Stop services started by Local Agent Stack before installing an app update".into(),
        );
    }
    let update = pending
        .0
        .lock()
        .map_err(|_| "the pending update lock is unavailable".to_string())?
        .take()
        .ok_or_else(|| "Check for an update before installing it".to_string())?;
    let progress_app = app.clone();
    let finished_app = app.clone();
    let downloaded = Arc::new(AtomicU64::new(0));
    let progress_downloaded = downloaded.clone();
    update
        .download_and_install(
            move |chunk_length, content_length| {
                let completed = progress_downloaded
                    .fetch_add(chunk_length as u64, Ordering::Relaxed)
                    .saturating_add(chunk_length as u64);
                let _ = progress_app.emit(
                    "app-update-progress",
                    AppUpdateProgress {
                        downloaded: completed,
                        total: content_length,
                        message: "Downloading and verifying the signed update".into(),
                    },
                );
            },
            move || {
                let completed = downloaded.load(Ordering::Relaxed);
                let _ = finished_app.emit(
                    "app-update-progress",
                    AppUpdateProgress {
                        downloaded: completed,
                        total: Some(completed),
                        message: "Signature verified; installing the update".into(),
                    },
                );
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    app.restart()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let supervisor = tauri::async_runtime::block_on(StackSupervisor::discover())
        .expect("failed to initialize the Local Agent Stack supervisor");
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState(supervisor))
        .manage(PendingUpdate(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_config,
            save_config,
            start_service,
            stop_service,
            restart_service,
            pull_model,
            unload_model,
            unload_all_models,
            delete_model,
            prepare_harness_profile,
            install_harness_companion,
            install_managed_ollama,
            rollback_managed_ollama,
            install_managed_harness,
            rollback_managed_harness,
            complete_setup,
            export_diagnostics,
            check_for_app_update,
            install_app_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Local Agent Stack");
}
