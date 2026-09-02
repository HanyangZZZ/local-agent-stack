mod tray;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use local_stack_core::{
    ActionResult, RuntimeInstallProgress, ServiceKind, ServiceLogTail, StackConfig, StackSnapshot,
    StackSupervisor, TraceReplay, TraceSessionSummary, TraceStore,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::tray::TrayControlState;

struct AppState(StackSupervisor);
struct TraceState(Arc<tokio::sync::RwLock<TraceStore>>);
struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Default)]
pub(crate) struct ControlGate(tokio::sync::Mutex<()>);

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
async fn get_service_log(
    service: &str,
    state: State<'_, AppState>,
) -> Result<ServiceLogTail, String> {
    state
        .0
        .service_log_tail(parse_service(service)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_config(
    config: StackConfig,
    state: State<'_, AppState>,
    trace: State<'_, TraceState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    let trace_config = config.trace.clone();
    let result = state
        .0
        .save_config(config)
        .await
        .map_err(|error| error.to_string())?;
    let replacement = TraceStore::discover(
        trace_config.enabled,
        trace_config.session_root.as_deref(),
        trace_config.inference_slots,
    )
    .map_err(|error| error.to_string())?;
    *trace.0.write().await = replacement;
    Ok(result)
}

#[tauri::command]
async fn list_trace_sessions(
    trace: State<'_, TraceState>,
) -> Result<Vec<TraceSessionSummary>, String> {
    trace
        .0
        .read()
        .await
        .list_sessions()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn load_trace_session(
    session_id: &str,
    trace: State<'_, TraceState>,
) -> Result<TraceReplay, String> {
    trace
        .0
        .read()
        .await
        .load_session(session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_service(
    service: &str,
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .start(parse_service(service)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_service(
    service: &str,
    app: AppHandle,
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    let kind = parse_service(service)?;
    let result = state
        .0
        .stop(kind)
        .await
        .map_err(|error| error.to_string())?;
    if kind == ServiceKind::Harness
        && let Some(window) = app.get_webview_window("harness")
    {
        let _ = window.close();
    }
    Ok(result)
}

#[tauri::command]
async fn restart_service(
    service: &str,
    app: AppHandle,
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    let kind = parse_service(service)?;
    if kind == ServiceKind::Harness
        && let Some(window) = app.get_webview_window("harness")
    {
        let _ = window.close();
    }
    let result = state
        .0
        .restart(kind)
        .await
        .map_err(|error| error.to_string())?;
    if kind == ServiceKind::Harness {
        show_harness_window(&app, &state.0).await?;
    }
    Ok(result)
}

async fn show_harness_window(app: &AppHandle, supervisor: &StackSupervisor) -> Result<(), String> {
    let snapshot = supervisor
        .snapshot()
        .await
        .map_err(|error| error.to_string())?;
    if snapshot.harness.state != local_stack_core::ServiceState::Online {
        return Err("Harness is offline; start it before opening the workspace".into());
    }
    let launch_url = snapshot.harness.launch_url.ok_or_else(|| {
        snapshot
            .harness
            .message
            .unwrap_or_else(|| "Harness did not publish an authenticated workspace URL".into())
    })?;
    let url = launch_url
        .parse::<tauri::Url>()
        .map_err(|error| format!("invalid Harness launch URL: {error}"))?;
    if let Some(window) = app.get_webview_window("harness") {
        window.navigate(url).map_err(|error| error.to_string())?;
        window.show().map_err(|error| error.to_string())?;
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }

    let allowed_scheme = url.scheme().to_owned();
    let allowed_host = url.host_str().map(str::to_owned);
    let allowed_port = url.port_or_known_default();
    WebviewWindowBuilder::new(app, "harness", WebviewUrl::External(url))
        .title("DeepSeek Harness")
        .inner_size(1280.0, 820.0)
        .min_inner_size(920.0, 640.0)
        .center()
        .zoom_hotkeys_enabled(true)
        .enable_clipboard_access()
        .on_navigation(move |candidate| {
            candidate.scheme() == allowed_scheme
                && candidate.host_str() == allowed_host.as_deref()
                && candidate.port_or_known_default() == allowed_port
        })
        .build()
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn open_harness(app: AppHandle, state: State<'_, AppState>) -> Result<ActionResult, String> {
    show_harness_window(&app, &state.0).await?;
    Ok(ActionResult::success("Harness workspace opened"))
}

#[tauri::command]
async fn start_stack(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .start_stack()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_managed_stack(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .stop_managed_stack()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pull_model(
    model: &str,
    app: AppHandle,
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .pull_model_with_progress(model, move |progress| {
            let _ = app.emit("ollama-pull-progress", progress);
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn unload_model(
    model: &str,
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .unload_model(model)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn unload_all_models(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .unload_all_models()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_model(
    model: &str,
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .delete_model(model)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn prepare_harness_profile(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .prepare_harness_profile()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn install_harness_companion(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
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
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .install_managed_ollama_with_progress(move |progress: RuntimeInstallProgress| {
            let _ = app.emit("runtime-install-progress", progress);
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn rollback_managed_ollama(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .rollback_managed_ollama()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn install_managed_harness(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .install_managed_harness()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn rollback_managed_harness(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
    state
        .0
        .rollback_managed_harness()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn complete_setup(
    state: State<'_, AppState>,
    gate: State<'_, ControlGate>,
) -> Result<ActionResult, String> {
    let _guard = gate.0.lock().await;
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
    gate: State<'_, ControlGate>,
) -> Result<(), String> {
    let _guard = gate.0.lock().await;
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
    let trace_config = tauri::async_runtime::block_on(supervisor.config()).trace;
    let trace_store = TraceStore::discover(
        trace_config.enabled,
        trace_config.session_root.as_deref(),
        trace_config.inference_slots,
    )
    .expect("failed to initialize the Local Agent Stack trace recorder");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState(supervisor))
        .manage(TraceState(Arc::new(tokio::sync::RwLock::new(trace_store))))
        .manage(PendingUpdate(Mutex::new(None)))
        .manage(ControlGate::default())
        .manage(TrayControlState::default())
        .setup(|app| {
            tray::install(app)?;
            let handle = app.handle().clone();
            let supervisor = app.state::<AppState>().0.clone();
            let telemetry_supervisor = supervisor.clone();
            let trace = app.state::<TraceState>().0.clone();
            tauri::async_runtime::spawn(async move {
                let _ = show_harness_window(&handle, &supervisor).await;
            });
            tauri::async_runtime::spawn(async move {
                loop {
                    let config = telemetry_supervisor.config().await;
                    let recorder = trace.read().await.clone();
                    if recorder.enabled() {
                        let _ = recorder.record_telemetry(&config.ollama.url).await;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(
                        config.trace.gpu_sample_interval_ms,
                    ))
                    .await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_config,
            get_service_log,
            list_trace_sessions,
            load_trace_session,
            save_config,
            start_service,
            stop_service,
            restart_service,
            open_harness,
            start_stack,
            stop_managed_stack,
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
