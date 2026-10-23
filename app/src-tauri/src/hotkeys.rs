//! Global shortcuts. Registration failures are reported per shortcut
//! (`MC-HK-001`) so the settings page can mark them in red.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use meetcue_core::ipc::{AppEvent, HotkeyState};
use meetcue_core::{codes, FocusTarget, MeetCueEvent, UserAction};

use crate::state::AppState;
use crate::{session, windows, APP_EVENT, PIPELINE_EVENT};

#[derive(Clone, Copy)]
enum Action {
    Toggle,
    Cycle,
    Prev,
    Next,
    Answered,
}

fn run(app: &AppHandle, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        match action {
            Action::Toggle => {
                if let Err(e) = session::toggle(&app).await {
                    let _ = app.emit(
                        PIPELINE_EVENT,
                        MeetCueEvent::Error {
                            code: e.code,
                            detail: e.detail,
                        },
                    );
                }
            }
            Action::Cycle => windows::cycle_mode(&app),
            Action::Prev => {
                state
                    .backend_send(UserAction::FocusStep {
                        target: FocusTarget::Prev,
                    })
                    .await
            }
            Action::Next => {
                if state.practice.lock().unwrap().is_some() {
                    crate::practice::control(&app, crate::practice::Control::Skip);
                } else {
                    state
                        .backend_send(UserAction::FocusStep {
                            target: FocusTarget::Next,
                        })
                        .await
                }
            }
            Action::Answered => {
                if state.practice.lock().unwrap().is_some() {
                    crate::practice::control(&app, crate::practice::Control::Answered);
                }
                state.backend_send(UserAction::MarkAnswered).await
            }
        }
    });
}

impl AppState {
    pub async fn backend_send(&self, action: UserAction) {
        use meetcue_pipeline::HintBackend;
        self.backend.send(action).await;
    }
}

/// (Re)registers every shortcut from the settings and publishes their status.
pub fn register_all(app: &AppHandle) -> Vec<HotkeyState> {
    let state = app.state::<AppState>();
    let keys = state.settings().hotkeys;
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let list = [
        ("toggleCapture", keys.toggle_capture, Action::Toggle),
        ("cyclePanel", keys.cycle_panel, Action::Cycle),
        ("focusPrev", keys.focus_prev, Action::Prev),
        ("focusNext", keys.focus_next, Action::Next),
        ("markAnswered", keys.mark_answered, Action::Answered),
    ];
    let mut status = Vec::new();
    for (name, accel, action) in list {
        let result = gs.on_shortcut(accel.as_str(), move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                run(app, action);
            }
        });
        if let Err(e) = &result {
            tracing::warn!("shortcut {accel} could not be registered: {e}");
        }
        status.push(HotkeyState {
            action: name.into(),
            accelerator: accel,
            ok: result.is_ok(),
        });
    }
    if status.iter().any(|s| !s.ok) {
        let failed: Vec<_> = status
            .iter()
            .filter(|s| !s.ok)
            .map(|s| s.accelerator.clone())
            .collect();
        let _ = app.emit(
            APP_EVENT,
            AppEvent::Notice {
                code: codes::HOTKEY_TAKEN.into(),
                detail: failed.join(", "),
            },
        );
    }
    *state.hotkeys.lock().unwrap() = status.clone();
    let _ = app.emit(
        APP_EVENT,
        AppEvent::HotkeyStatus {
            hotkeys: status.clone(),
        },
    );
    status
}
