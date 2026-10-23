//! Tray icon and its localized menu.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use meetcue_core::SessionState;

use crate::i18n::t;
use crate::state::{AppState, DOCS_URL};
use crate::{diagnostics, session, windows};

const TRAY_ID: &str = "main";

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let lang = state.language();
    let toggle_label = match state.session_state() {
        SessionState::Running => t(lang, "tray.pause"),
        SessionState::Paused => t(lang, "tray.resume"),
        _ => t(lang, "tray.start"),
    };
    let item = |id: &str, key: &str| MenuItem::with_id(app, id, t(lang, key), true, None::<&str>);
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "toggle", toggle_label, true, None::<&str>)?,
            &item("panel", "tray.panel")?,
            &PredefinedMenuItem::separator(app)?,
            &item("settings", "tray.settings")?,
            &item("practice", "tray.practice")?,
            &item("reset", "tray.resetPanel")?,
            &item("diagnostics", "tray.exportDiagnostics")?,
            &item("help", "tray.help")?,
            &PredefinedMenuItem::separator(app)?,
            &item("quit", "tray.quit")?,
        ],
    )
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("MeetCue")
        .menu(&menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let app = app.clone();
            match event.id.as_ref() {
                "toggle" => {
                    tauri::async_runtime::spawn(async move {
                        let _ = session::toggle(&app).await;
                    });
                }
                "panel" => windows::cycle_mode(&app),
                "settings" => {
                    let _ = windows::open_view(&app, "settings", None);
                }
                "practice" => {
                    let _ = windows::open_view(&app, "practice", None);
                }
                "reset" => windows::reset_position(&app),
                "diagnostics" => {
                    tauri::async_runtime::spawn(async move {
                        let _ = diagnostics::export_interactive(&app).await;
                    });
                }
                "help" => {
                    use tauri_plugin_opener::OpenerExt;
                    let lang = app.state::<AppState>().language();
                    let url = if lang.starts_with("zh") {
                        format!("{DOCS_URL}zh/")
                    } else {
                        DOCS_URL.to_string()
                    };
                    let _ = app.opener().open_url(url, None::<&str>);
                }
                "quit" => {
                    tauri::async_runtime::spawn(async move {
                        session::stop(&app).await;
                        windows::persist(&app);
                        app.exit(0);
                    });
                }
                _ => {}
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Rebuilds the menu after a language or session state change.
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(m)) = (app.tray_by_id(TRAY_ID), menu(app)) {
        let _ = tray.set_menu(Some(m));
    }
}
