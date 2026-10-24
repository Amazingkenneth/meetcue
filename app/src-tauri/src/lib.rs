//! The MeetCue desktop shell: windows, tray, global shortcuts and the bridge
//! between the interface and the pipeline.

pub mod cli;
mod command_names;
mod config;
pub mod console;
mod diagnostics;
mod hotkeys;
mod i18n;
mod ipc;
mod logging;
mod practice;
mod prices;
mod selftest;
mod session;
mod state;
mod tray;
mod usage;
mod windows;

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tauri::{Emitter, Manager, RunEvent};

use meetcue_core::ipc::AppEvent;
use meetcue_core::{codes, ErrorCatalog, SessionState};
use meetcue_pipeline::probe::ProbeReport;
use meetcue_pipeline::LocalHintBackend;
use meetcue_platform::credentials::{CredentialStore, MemoryCredentials, SystemCredentials};
use meetcue_platform::paths;

use state::{AppState, DOCS_URL};

/// Pipeline events (`MeetCueEvent`).
pub const PIPELINE_EVENT: &str = "meetcue://event";
/// Application events (`AppEvent`).
pub const APP_EVENT: &str = "meetcue://app";

fn load_probes(path: &std::path::Path) -> Vec<ProbeReport> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Runs the application and returns the process exit code.
pub fn run(opts: cli::Options) -> i32 {
    let paths = paths::resolve();
    let _ = paths.ensure();
    let settings = config::load(&paths.settings_file());
    let _log_guard = logging::init(&paths.logs_dir(), settings.diagnostics.debug_logging);
    tracing::info!(
        "MeetCue {} starting (portable: {})",
        meetcue_core::VERSION,
        paths.portable
    );

    let system_locale = meetcue_platform::system_locale();
    if let Err(e) = tauri::webview_version() {
        let lang = settings.general.ui_language.resolve(&system_locale);
        let info = ErrorCatalog::builtin()
            .get(codes::SYS_WEBVIEW2)
            .expect("catalog entry");
        let url = ErrorCatalog::builtin().docs_url(DOCS_URL, codes::SYS_WEBVIEW2, lang);
        tracing::error!("webview unavailable: {e}");
        console::fatal_box(
            "MeetCue",
            &format!(
                "{}\n\n{}\n\n{} · {url}",
                info.title.get(lang),
                info.fix.get(lang),
                codes::SYS_WEBVIEW2
            ),
        );
        return 1;
    }

    let credentials: Arc<dyn CredentialStore> = if opts.demo {
        Arc::new(MemoryCredentials::default())
    } else {
        Arc::new(SystemCredentials)
    };
    let fell_back = paths.fell_back;
    let wizard_needed = !settings.general.wizard_completed && !opts.demo && !opts.self_test;
    let state = AppState {
        probes: Mutex::new(load_probes(&paths.latency_history())),
        paths,
        settings: RwLock::new(settings),
        credentials,
        backend: Arc::new(LocalHintBackend::new()),
        audio: Mutex::new(None),
        preview: Mutex::new(None),
        session: Mutex::new(SessionState::Idle),
        demo: opts.demo,
        self_test: opts.self_test,
        system_locale,
        hotkeys: Mutex::new(Vec::new()),
        practice: Mutex::new(None),
    };
    prices::load_cached(&state);

    let exit_code = Arc::new(Mutex::new(0));
    let exit_for_setup = exit_code.clone();
    let mut builder = tauri::Builder::default();
    if !opts.self_test {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::set_mode(app, meetcue_core::settings::PanelMode::Full);
        }));
    }
    let app = builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(state)
        .invoke_handler(ipc::handler())
        .setup(move |app| {
            let handle = app.handle().clone();
            session::spawn_forwarder(&handle);
            hotkeys::register_all(&handle);
            tray::create(&handle)?;
            windows::create_overlay(&handle)?;
            if !opts.self_test {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = prices::refresh(&h, true).await {
                        tracing::warn!("could not update the price list: {e}");
                    }
                });
            }
            if wizard_needed {
                windows::open_view(&handle, "wizard", None)?;
            }
            if fell_back {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let _ = h.emit(
                        APP_EVENT,
                        AppEvent::Notice {
                            code: codes::SYS_CONFIG_DIR.into(),
                            detail: String::new(),
                        },
                    );
                });
            }
            if opts.demo {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    if let Err(e) = session::start(&h).await {
                        tracing::error!("demo failed to start: {e}");
                    }
                });
            }
            if opts.self_test {
                let h = handle.clone();
                let exit = exit_for_setup.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    let report = selftest::run(&h).await;
                    print!("{report}");
                    let state = h.state::<AppState>();
                    let _ = std::fs::write(state.paths.config_dir.join("self-test.txt"), &report);
                    *exit.lock().unwrap() = i32::from(report.contains("[FAIL]"));
                    h.exit(0);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the panel hides it; MeetCue keeps running in the tray.
            if window.label() == windows::OVERLAY {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("could not start: {e}");
            console::fatal_box("MeetCue", &e.to_string());
            return 1;
        }
    };
    app.run(|handle, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() {
                // Last window closed: keep running in the tray.
                api.prevent_exit();
            }
        }
        RunEvent::Exit => windows::persist(handle),
        _ => {}
    });
    let code = *exit_code.lock().unwrap();
    code
}

#[cfg(test)]
mod tests {
    use super::command_names::COMMANDS;
    use std::collections::BTreeSet;

    #[test]
    fn every_command_is_allowed_by_the_capability() {
        let cap: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let allowed: BTreeSet<String> = cap["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .filter_map(|p| p.strip_prefix("allow-"))
            .map(|p| p.replace('-', "_"))
            .collect();
        let commands: BTreeSet<String> = COMMANDS.iter().map(|c| c.to_string()).collect();
        assert_eq!(allowed, commands);
    }

    #[test]
    fn interface_and_shell_list_the_same_commands() {
        // Checkouts may use CRLF line endings on Windows.
        let ts = include_str!("../../ui/src/backend/types.ts").replace("\r\n", "\n");
        let start = ts.find("export interface Commands {").unwrap();
        let end = start + ts[start..].find("\n}\n").unwrap();
        let ui: BTreeSet<String> = ts[start..end]
            .lines()
            .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
            .filter_map(|l| {
                l.trim()
                    .split_once(": {")
                    .map(|(k, _)| k.trim().to_string())
            })
            .collect();
        let commands: BTreeSet<String> = COMMANDS.iter().map(|c| c.to_string()).collect();
        assert_eq!(ui, commands);
    }

    #[test]
    fn handler_registers_every_command() {
        let src = include_str!("ipc.rs");
        let start = src.find("tauri::generate_handler![").unwrap();
        let body = &src[start..start + src[start..].find(']').unwrap()];
        for c in COMMANDS {
            assert!(
                body.contains(&format!("        {c},")),
                "{c} missing from generate_handler!"
            );
        }
    }
}
