//! Global shortcuts. Registration failures are reported per shortcut
//! (`MC-HK-001`) so the settings page can mark them in red.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use meetcue_core::ipc::{AppEvent, HotkeyState, PanelChange};
use meetcue_core::{codes, FocusTarget, UserAction};

use crate::state::AppState;
use crate::windows::Direction;
use crate::{session, windows, APP_EVENT};

#[derive(Clone, Copy)]
enum Action {
    Toggle,
    Cycle,
    Prev,
    Next,
    Answered,
    Move(windows::Direction),
    Spot(u8),
    Peek,
    ClickThrough,
}

fn run(app: &AppHandle, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        match action {
            Action::Toggle => {
                let _ = session::reported(&app, session::toggle(&app).await);
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
            // Handled in the shortcut callback, which also sees the release.
            Action::Move(_) => {}
            Action::Spot(slot) => windows::change_panel(&app, PanelChange::GoSpot { slot }),
            Action::Peek => windows::change_panel(&app, PanelChange::Peek),
            Action::ClickThrough => windows::toggle_click_through(&app),
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
        ("moveUp", keys.move_up, Action::Move(Direction::Up)),
        ("moveDown", keys.move_down, Action::Move(Direction::Down)),
        ("moveLeft", keys.move_left, Action::Move(Direction::Left)),
        ("moveRight", keys.move_right, Action::Move(Direction::Right)),
        ("spot1", keys.spot1, Action::Spot(0)),
        ("spot2", keys.spot2, Action::Spot(1)),
        ("spot3", keys.spot3, Action::Spot(2)),
        ("peek", keys.peek, Action::Peek),
        ("clickThrough", keys.click_through, Action::ClickThrough),
    ];
    let mut status = Vec::new();
    for (name, accel, action) in list {
        let result = gs.on_shortcut(accel.as_str(), move |app, _shortcut, event| {
            let pressed = event.state == ShortcutState::Pressed;
            match action {
                // Moving also needs the release, to stop when the keys are let go.
                Action::Move(dir) => windows::move_key(app, dir, pressed),
                _ if pressed => run(app, action),
                _ => {}
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
