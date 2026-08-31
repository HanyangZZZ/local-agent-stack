use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{
    App, AppHandle, Emitter, Manager, Wry,
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{AppState, ControlGate};

const TRAY_ID: &str = "local-agent-stack-tray";
const SHOW_ID: &str = "tray-show";
const START_STACK_ID: &str = "tray-start-stack";
const STOP_STACK_ID: &str = "tray-stop-stack";
const RELEASE_VRAM_ID: &str = "tray-release-vram";
const QUIT_ID: &str = "tray-quit";

pub(crate) const TRAY_ACTION_EVENT: &str = "tray-action-result";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayCommand {
    Show,
    StartStack,
    StopManagedStack,
    ReleaseVram,
    Quit,
}

impl TrayCommand {
    fn from_menu_id(id: &str) -> Option<Self> {
        match id {
            SHOW_ID => Some(Self::Show),
            START_STACK_ID => Some(Self::StartStack),
            STOP_STACK_ID => Some(Self::StopManagedStack),
            RELEASE_VRAM_ID => Some(Self::ReleaseVram),
            QUIT_ID => Some(Self::Quit),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::StartStack => "startStack",
            Self::StopManagedStack => "stopManagedStack",
            Self::ReleaseVram => "releaseVram",
            Self::Quit => "quit",
        }
    }
}

#[derive(Default)]
pub(crate) struct TrayControlState {
    busy: AtomicBool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrayActionFeedback {
    action: &'static str,
    ok: bool,
    message: String,
}

pub(crate) fn install(app: &App<Wry>) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(SHOW_ID, "Show Local Agent Stack")
        .separator()
        .text(START_STACK_ID, "Start stack")
        .text(STOP_STACK_ID, "Stop managed stack")
        .text(RELEASE_VRAM_ID, "Release all Ollama VRAM")
        .separator()
        .text(QUIT_ID, "Quit and stop managed services")
        .build()?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Local Agent Stack")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu_event(app, &event.id().0))
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

pub(crate) fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn handle_menu_event(app: &AppHandle<Wry>, menu_id: &str) {
    let Some(command) = TrayCommand::from_menu_id(menu_id) else {
        return;
    };
    if command == TrayCommand::Show {
        show_main_window(app);
        return;
    }

    let control = app.state::<TrayControlState>();
    if control
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        emit_feedback(
            app,
            TrayActionFeedback {
                action: command.label(),
                ok: false,
                message: "Another tray action is already running".into(),
            },
        );
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let gate = app.state::<ControlGate>();
        let Ok(_guard) = gate.0.try_lock() else {
            app.state::<TrayControlState>()
                .busy
                .store(false, Ordering::Release);
            emit_feedback(
                &app,
                TrayActionFeedback {
                    action: command.label(),
                    ok: false,
                    message: "Another control action is already running".into(),
                },
            );
            return;
        };
        let supervisor = app.state::<AppState>().0.clone();
        let outcome = match command {
            TrayCommand::StartStack => supervisor.start_stack().await,
            TrayCommand::StopManagedStack | TrayCommand::Quit => {
                supervisor.stop_managed_stack().await
            }
            TrayCommand::ReleaseVram => supervisor.unload_all_models().await,
            TrayCommand::Show => unreachable!("show is handled synchronously"),
        };

        let feedback = match outcome {
            Ok(result) => TrayActionFeedback {
                action: command.label(),
                ok: result.ok,
                message: result.message,
            },
            Err(error) => TrayActionFeedback {
                action: command.label(),
                ok: false,
                message: error.to_string(),
            },
        };
        let should_exit = command == TrayCommand::Quit && feedback.ok;
        app.state::<TrayControlState>()
            .busy
            .store(false, Ordering::Release);
        emit_feedback(&app, feedback.clone());

        if should_exit {
            app.exit(0);
        } else if !feedback.ok {
            show_main_window(&app);
        }
    });
}

fn emit_feedback(app: &AppHandle<Wry>, feedback: TrayActionFeedback) {
    let _ = app.emit(TRAY_ACTION_EVENT, &feedback);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let status = if feedback.ok {
            "Ready"
        } else {
            "Action failed"
        };
        let _ = tray.set_tooltip(Some(format!("Local Agent Stack — {status}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_declared_tray_menu_ids() {
        assert_eq!(TrayCommand::from_menu_id(SHOW_ID), Some(TrayCommand::Show));
        assert_eq!(
            TrayCommand::from_menu_id(START_STACK_ID),
            Some(TrayCommand::StartStack)
        );
        assert_eq!(
            TrayCommand::from_menu_id(STOP_STACK_ID),
            Some(TrayCommand::StopManagedStack)
        );
        assert_eq!(
            TrayCommand::from_menu_id(RELEASE_VRAM_ID),
            Some(TrayCommand::ReleaseVram)
        );
        assert_eq!(TrayCommand::from_menu_id(QUIT_ID), Some(TrayCommand::Quit));
        assert_eq!(TrayCommand::from_menu_id("tray-unknown"), None);
    }

    #[test]
    fn tray_action_labels_are_stable_frontend_values() {
        assert_eq!(TrayCommand::Show.label(), "show");
        assert_eq!(TrayCommand::StartStack.label(), "startStack");
        assert_eq!(TrayCommand::StopManagedStack.label(), "stopManagedStack");
        assert_eq!(TrayCommand::ReleaseVram.label(), "releaseVram");
        assert_eq!(TrayCommand::Quit.label(), "quit");
    }
}
