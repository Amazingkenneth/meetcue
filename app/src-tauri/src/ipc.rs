//! Commands the interface can call. The names are listed in
//! `command_names.rs`; each one needs a matching permission in
//! `capabilities/default.json` (checked by a test).

use std::collections::BTreeMap;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use meetcue_core::ipc::{AppEvent, AppInfo, DeviceList, HotkeyState, UsageSummary, VerifyResult};
use meetcue_core::pricing::{
    estimate_hourly, CostAssumptions, HourlyEstimate, PriceEntry, PriceTable,
};
use meetcue_core::settings::{EndpointChoice, Material, PanelMode};
use meetcue_core::{codes, MeetCueError, Settings, TimedEvent, UsagePurpose, UserAction};
use meetcue_pipeline::endpoints::EndpointCatalog;
use meetcue_pipeline::practice::{PracticeReport, PracticeReportSummary, PracticeScript};
use meetcue_pipeline::probe::{self, ProbeEnv, ProbeReport, ProbeRequest};
use meetcue_pipeline::provider::ModelInfo;
use meetcue_pipeline::{materials, providers, HintBackend};

use crate::state::{AppState, DOCS_URL};
use crate::usage::{self, LedgerLine};
use crate::{config, diagnostics, hotkeys, practice, session, tray, windows, APP_EVENT};

type Res<T> = Result<T, MeetCueError>;

fn io_err(e: impl std::fmt::Display) -> MeetCueError {
    MeetCueError::new(codes::SYS_CONFIG_DIR, e.to_string())
}

/// Provider ids whose credentials the interface may ask about.
const PROVIDER_IDS: &[&str] = &[
    "soniox",
    "deepgram",
    "anthropic",
    "bedrock",
    "openai-compatible",
];

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: meetcue_core::VERSION.into(),
        platform: std::env::consts::OS.into(),
        language: state.language().into(),
        system_locale: state.system_locale.clone(),
        portable: state.paths.portable,
        config_dir: state.paths.config_dir.to_string_lossy().into_owned(),
        demo: state.demo,
        docs_url: DOCS_URL.into(),
        capture_supported: cfg!(windows),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Res<Settings> {
    let old = state.settings();
    config::save(&state.paths.settings_file(), &settings).map_err(io_err)?;
    *state.settings.write().unwrap() = settings.clone();
    if old.hotkeys != settings.hotkeys {
        hotkeys::register_all(&app);
    }
    if old.general.ui_language != settings.general.ui_language {
        tray::refresh(&app);
    }
    if old.overlay.click_through != settings.overlay.click_through
        || old.overlay.hide_in_screen_share != settings.overlay.hide_in_screen_share
    {
        windows::apply_overlay_settings(&app);
    }
    if old.general.autostart != settings.general.autostart {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = app.autolaunch();
        let r = if settings.general.autostart {
            launcher.enable()
        } else {
            launcher.disable()
        };
        if let Err(e) = r {
            tracing::warn!("autostart: {e}");
        }
    }
    let _ = app.emit(
        APP_EVENT,
        AppEvent::SettingsChanged {
            settings: Box::new(settings.clone()),
        },
    );
    Ok(settings)
}

#[tauri::command]
pub async fn list_devices() -> Res<DeviceList> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = meetcue_audio::platform_source();
        Ok(DeviceList {
            microphones: s.list_microphones()?,
            outputs: s.list_outputs()?,
        })
    })
    .await
    .map_err(io_err)?
}

#[tauri::command]
pub fn credential_status(state: State<'_, AppState>) -> BTreeMap<String, bool> {
    PROVIDER_IDS
        .iter()
        .map(|p| (p.to_string(), state.credentials.has(p)))
        .collect()
}

#[tauri::command]
pub fn set_credential(state: State<'_, AppState>, key: String, value: String) -> Res<()> {
    if !PROVIDER_IDS.contains(&key.as_str()) {
        return Err(MeetCueError::new(
            codes::SYS_CREDENTIALS,
            "unknown credential",
        ));
    }
    state.credentials.set(&key, &value)
}

#[tauri::command]
pub fn delete_credential(state: State<'_, AppState>, key: String) -> Res<()> {
    state.credentials.delete(&key)
}

fn kind_of(provider: &str) -> &'static str {
    match provider {
        "soniox" | "deepgram" | "azure-speech" | "mock" => "asr",
        _ => "llm",
    }
}

fn config_for(state: &AppState, provider: &str) -> Res<providers::ProviderConfig> {
    let s = state.settings();
    let slot = if kind_of(provider) == "asr" {
        &s.providers.asr
    } else {
        &s.providers.hints
    };
    let model = if slot.provider.as_deref() == Some(provider) {
        slot.model.clone().unwrap_or_default()
    } else {
        String::new()
    };
    let choice = if slot.provider.as_deref() == Some(provider) {
        slot.endpoint.clone()
    } else {
        EndpointChoice::Auto
    };
    session::provider_config(state, &s, provider, &model, &choice)
}

#[tauri::command]
pub async fn verify_provider(state: State<'_, AppState>, provider: String) -> Res<VerifyResult> {
    let cfg = config_for(&state, &provider)?;
    let result = if kind_of(&provider) == "asr" {
        match providers::asr(&cfg) {
            Ok(p) => p.verify().await,
            Err(e) => Err(e),
        }
    } else {
        match providers::llm(&cfg) {
            Ok(p) => p.list_models().await.map(|_| ()),
            Err(e) => Err(e),
        }
    };
    Ok(match result {
        Ok(()) => VerifyResult::ok(),
        Err(e) => VerifyResult::err(&e),
    })
}

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>, provider: String) -> Res<Vec<ModelInfo>> {
    let cfg = config_for(&state, &provider)?;
    if kind_of(&provider) == "asr" {
        providers::asr(&cfg)?.list_models().await
    } else {
        providers::llm(&cfg)?.list_models().await
    }
}

#[tauri::command]
pub fn get_pricing() -> PriceTable {
    PriceTable::builtin().clone()
}

#[tauri::command]
pub fn estimate_cost(
    asr: Option<PriceEntry>,
    detect: Option<PriceEntry>,
    hints: Option<PriceEntry>,
    assumptions: CostAssumptions,
) -> HourlyEstimate {
    estimate_hourly(asr.as_ref(), detect.as_ref(), hints.as_ref(), &assumptions)
}

#[tauri::command]
pub fn provider_hosts(provider: String) -> Vec<String> {
    EndpointCatalog::builtin().hosts(&provider).to_vec()
}

#[tauri::command]
pub fn audio_preview(app: AppHandle, on: bool) -> Res<()> {
    session::audio_preview(&app, on)
}

#[tauri::command]
pub async fn start_session(app: AppHandle) -> Res<()> {
    session::start(&app).await
}

#[tauri::command]
pub async fn stop_session(app: AppHandle) -> Res<()> {
    session::stop(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn toggle_capture(app: AppHandle) -> Res<()> {
    session::toggle(&app).await
}

#[tauri::command]
pub async fn user_action(state: State<'_, AppState>, action: UserAction) -> Res<()> {
    state.backend.send(action).await;
    Ok(())
}

#[tauri::command]
pub fn session_history(state: State<'_, AppState>) -> Vec<TimedEvent> {
    state.backend.history()
}

#[tauri::command]
pub fn set_panel_mode(app: AppHandle, mode: PanelMode) {
    windows::set_mode(&app, mode);
}

#[tauri::command]
pub fn set_always_on_top(app: AppHandle, on: bool) -> Res<()> {
    if let Some(w) = app.get_webview_window(windows::OVERLAY) {
        w.set_always_on_top(on).map_err(io_err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_window(app: AppHandle, view: String, section: Option<String>) -> Res<()> {
    const VIEWS: &[&str] = &["settings", "wizard", "practice", "review"];
    if !VIEWS.contains(&view.as_str()) {
        return Err(io_err("unknown view"));
    }
    windows::open_view(&app, &view, section.as_deref()).map_err(io_err)
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Res<()> {
    // Only web links and the system settings pages named in the error catalog.
    let allowed = url.starts_with("https://") || url.starts_with("ms-settings:");
    if !allowed {
        return Err(io_err("blocked URL"));
    }
    app.opener().open_url(url, None::<&str>).map_err(io_err)
}

#[tauri::command]
pub fn hotkey_status(state: State<'_, AppState>) -> Vec<HotkeyState> {
    state.hotkeys.lock().unwrap().clone()
}

fn probe_audio() -> Vec<i16> {
    meetcue_pipeline::textio::wav_pcm16(include_bytes!("../../../samples/audio/probe-zh-en.wav"))
        .unwrap_or_default()
}

#[tauri::command]
pub async fn run_probe(app: AppHandle, request: ProbeRequest) -> Res<ProbeReport> {
    let state = app.state::<AppState>();
    let s = state.settings();
    let resolve_app = app.clone();
    let progress_app = app.clone();
    let usage_app = app.clone();
    let env = ProbeEnv {
        resolve: std::sync::Arc::new(move |target, host| {
            let state = resolve_app.state::<AppState>();
            let mut cfg = config_for(&state, &target.provider)?;
            if let Some(m) = &target.model {
                cfg.model = m.clone();
            }
            if cfg.model.is_empty() {
                // Use the cheapest priced model of the provider for timing.
                cfg.model = PriceTable::builtin()
                    .entries
                    .iter()
                    .filter(|e| e.provider == target.provider)
                    .min_by(|a, b| {
                        a.input
                            .or(a.audio)
                            .partial_cmp(&b.input.or(b.audio))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|e| e.model.clone())
                    .unwrap_or_default();
            }
            if let Some(h) = host {
                if target.provider != "openai-compatible" {
                    cfg.host = Some(h.to_string());
                }
            }
            Ok(cfg)
        }),
        progress: std::sync::Arc::new(move |done, total, label| {
            let _ = progress_app.emit(
                APP_EVENT,
                AppEvent::ProbeProgress {
                    done,
                    total,
                    label: label.into(),
                },
            );
        }),
        on_usage: std::sync::Arc::new(move |provider, model, purpose: UsagePurpose, usage| {
            let state = usage_app.state::<AppState>();
            let s = state.settings();
            let micros = PriceTable::builtin()
                .find(&s.providers.pricing_overrides, provider, model)
                .map(|p| p.cost(&usage));
            let _ = usage::append(
                &state.paths.usage_ledger(),
                &LedgerLine {
                    at: chrono::Utc::now().to_rfc3339(),
                    provider: provider.into(),
                    model: model.into(),
                    purpose,
                    usage,
                    micros,
                },
            );
        }),
        assumptions: s.providers.estimate.clone(),
        overrides: s.providers.pricing_overrides.clone(),
        sentence_wait_ms: s.audio.endpoint_silence_ms as f64,
        audio: std::sync::Arc::new(probe_audio()),
        proxy: s.providers.proxy.clone(),
    };
    let report = probe::run(&request, env).await;
    {
        let mut probes = state.probes.lock().unwrap();
        probes.insert(0, report.clone());
        probes.truncate(50);
        let json = serde_json::to_string_pretty(&*probes).unwrap_or_default();
        let _ = config::write_atomic(&state.paths.latency_history(), json.as_bytes());
    }
    Ok(report)
}

#[tauri::command]
pub fn probe_history(state: State<'_, AppState>) -> Vec<ProbeReport> {
    state.probes.lock().unwrap().clone()
}

#[tauri::command]
pub fn usage_summary(state: State<'_, AppState>, month: Option<String>) -> UsageSummary {
    let month = month.unwrap_or_else(|| chrono::Utc::now().format("%Y-%m").to_string());
    usage::summary(&usage::read(&state.paths.usage_ledger()), &month)
}

async fn save_dialog(
    app: &AppHandle,
    name: String,
    filter: (&'static str, &'static [&'static str]),
) -> Option<std::path::PathBuf> {
    let dialog = app
        .dialog()
        .file()
        .set_file_name(&name)
        .add_filter(filter.0, filter.1);
    tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
}

#[tauri::command]
pub async fn export_usage_csv(app: AppHandle) -> Res<Option<String>> {
    let Some(path) = save_dialog(&app, "meetcue-usage.csv".into(), ("CSV", &["csv"])).await else {
        return Ok(None);
    };
    let state = app.state::<AppState>();
    std::fs::write(&path, usage::csv(&usage::read(&state.paths.usage_ledger()))).map_err(io_err)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn diagnostics_preview(app: AppHandle) -> Vec<String> {
    diagnostics::preview(&app)
}

#[tauri::command]
pub async fn export_diagnostics(app: AppHandle) -> Res<Option<String>> {
    diagnostics::export_interactive(&app).await
}

#[tauri::command]
pub async fn import_material(app: AppHandle) -> Res<Option<Material>> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("Documents", &["txt", "md", "docx", "pdf"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(io_err)?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes =
        std::fs::read(&path).map_err(|e| MeetCueError::new(codes::PRACTICE_FILE, e.to_string()))?;
    let text = tauri::async_runtime::spawn_blocking({
        let name = name.clone();
        move || materials::extract_text(&name, &bytes)
    })
    .await
    .map_err(io_err)??;
    Ok(Some(Material {
        name,
        path: path.to_string_lossy().into_owned(),
        text,
    }))
}

#[tauri::command]
pub async fn practice_import(app: AppHandle) -> Res<Option<PracticeScript>> {
    practice::import(&app).await
}

#[tauri::command]
pub async fn practice_classify(app: AppHandle, script: PracticeScript) -> Res<PracticeScript> {
    practice::classify(&app, script).await
}

#[tauri::command]
pub async fn practice_start(app: AppHandle, script: PracticeScript) -> Res<()> {
    practice::start(&app, script).await
}

#[tauri::command]
pub fn practice_control(app: AppHandle, control: String) -> Res<()> {
    let c = match control.as_str() {
        "pause" => practice::Control::Pause,
        "resume" => practice::Control::Resume,
        "skip" => practice::Control::Skip,
        "stop" => practice::Control::Stop,
        "answered" => practice::Control::Answered,
        _ => return Err(io_err("unknown control")),
    };
    practice::control(&app, c);
    Ok(())
}

#[tauri::command]
pub fn practice_reports(app: AppHandle) -> Vec<PracticeReportSummary> {
    practice::reports(&app)
}

#[tauri::command]
pub fn practice_report(app: AppHandle, id: String) -> Res<PracticeReport> {
    practice::report(&app, &id)
}

#[tauri::command]
pub async fn export_report(app: AppHandle, id: String) -> Res<Option<String>> {
    practice::export(&app, &id).await
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool {
    tauri::generate_handler![
        get_app_info,
        get_settings,
        save_settings,
        list_devices,
        credential_status,
        set_credential,
        delete_credential,
        verify_provider,
        list_models,
        get_pricing,
        estimate_cost,
        provider_hosts,
        audio_preview,
        start_session,
        stop_session,
        toggle_capture,
        user_action,
        session_history,
        set_panel_mode,
        set_always_on_top,
        open_window,
        open_url,
        hotkey_status,
        run_probe,
        probe_history,
        usage_summary,
        export_usage_csv,
        diagnostics_preview,
        export_diagnostics,
        import_material,
        practice_import,
        practice_classify,
        practice_start,
        practice_control,
        practice_reports,
        practice_report,
        export_report,
    ]
}
