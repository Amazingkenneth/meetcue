//! Commands the interface can call. The names are listed in
//! `command_names.rs`; each one needs a matching permission in
//! `capabilities/default.json` (checked by a test).

use std::collections::BTreeMap;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use meetcue_core::ipc::{
    AppEvent, AppInfo, DeviceList, HotkeyState, LogLine, PanelChange, UsageSummary, VerifyResult,
    Voice,
};
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
use meetcue_platform::speech::Speaker;

use crate::state::{AppState, DOCS_URL};
use crate::usage::{self, LedgerLine};
use crate::{config, diagnostics, hotkeys, practice, prices, session, tray, windows, APP_EVENT};

type Res<T> = Result<T, MeetCueError>;

/// A failure inside the app (a window, a dialog, a worker thread). These
/// used to carry MC-SYS-002, which tells the user their settings folder is
/// read-only: misleading, since none of them involve that folder.
fn io_err(e: impl std::fmt::Display) -> MeetCueError {
    tracing::warn!("command failed: {e}");
    MeetCueError::new(codes::SYS_INTERNAL, e.to_string())
}

/// Provider ids whose credentials the interface may ask about.
const PROVIDER_IDS: &[&str] = &["soniox", "deepgram", "anthropic", "openai-compatible"];

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
        capture_supported: cfg!(any(windows, target_os = "macos")),
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
    // The panel's place, size and saved spots belong to the panel: a window
    // that loaded the settings earlier must not move it back.
    let mut settings = settings;
    settings.overlay.x = old.overlay.x;
    settings.overlay.y = old.overlay.y;
    settings.overlay.width = old.overlay.width;
    settings.overlay.height = old.overlay.height;
    settings.overlay.spots = old.overlay.spots.clone();
    settings.overlay.normalize();
    config::save(&state.paths.settings_file(), &settings).map_err(|e| {
        tracing::warn!("could not save settings: {e}");
        MeetCueError::new(codes::SYS_SETTINGS_SAVE, e.to_string())
    })?;
    *state.settings.write().unwrap() = settings.clone();
    // A new microphone or voice threshold: restart capture if it's running.
    if old.audio.mic_device != settings.audio.mic_device
        || old.audio.vad_threshold_db != settings.audio.vad_threshold_db
        || old.audio.remote_vad_threshold_db != settings.audio.remote_vad_threshold_db
        || old.audio.boost_quiet_voices != settings.audio.boost_quiet_voices
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { session::restart_audio(&app).await });
    }
    // A model, language or hint setting changed during a session: apply it now.
    if old.providers != settings.providers
        || old.audio.languages != settings.audio.languages
        || old.audio.endpoint_silence_ms != settings.audio.endpoint_silence_ms
        || old.hints != settings.hints
        || old.general.hint_language != settings.general.hint_language
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { session::apply_live(&app).await });
    }
    if old.hotkeys != settings.hotkeys {
        hotkeys::register_all(&app);
    }
    if old.general.ui_language != settings.general.ui_language
        || old.overlay.click_through != settings.overlay.click_through
    {
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
pub fn set_credential(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Res<()> {
    if !PROVIDER_IDS.contains(&key.as_str()) {
        return Err(MeetCueError::new(
            codes::SYS_CREDENTIALS,
            "unknown credential",
        ));
    }
    state.credentials.set(&key, &value)?;
    // A running session picks up the new key without restarting.
    tauri::async_runtime::spawn(async move { session::apply_live(&app).await });
    Ok(())
}

#[tauri::command]
pub fn delete_credential(app: AppHandle, state: State<'_, AppState>, key: String) -> Res<()> {
    state.credentials.delete(&key)?;
    tauri::async_runtime::spawn(async move { session::apply_live(&app).await });
    Ok(())
}

/// Erases everything MeetCue keeps on this computer: API keys, settings,
/// logs, practice reports, usage, latency history, cached prices, webview
/// storage and the autostart entry. Then restarts, so the app comes back as
/// a fresh install with the setup assistant.
#[tauri::command]
pub async fn wipe_all_data(app: AppHandle) -> Res<()> {
    tracing::warn!("erasing all data on request");
    if practice::is_active(&app) {
        practice::control(&app, practice::Control::Stop);
        *app.state::<AppState>().practice.lock().unwrap() = None;
    }
    session::stop(&app).await;
    // Give stopping runs a moment to finish their last writes.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let state = app.state::<AppState>();
    for id in PROVIDER_IDS {
        state.credentials.delete(id)?;
    }
    {
        use tauri_plugin_autostart::ManagerExt;
        if let Err(e) = app.autolaunch().disable() {
            tracing::warn!("autostart: {e}");
        }
    }
    for (label, window) in app.webview_windows() {
        if let Err(e) = window.clear_all_browsing_data() {
            tracing::warn!("webview data for {label} not cleared: {e}");
        }
    }
    let left = state.paths.wipe();
    if !left.is_empty() {
        let names: Vec<String> = left.iter().map(|p| p.display().to_string()).collect();
        return Err(io_err(format!("could not delete {}", names.join(", "))));
    }
    app.restart()
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
pub async fn list_models(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
) -> Res<Vec<ModelInfo>> {
    let cfg = config_for(&state, &provider)?;
    let models = if kind_of(&provider) == "asr" {
        providers::asr(&cfg)?.list_models().await?
    } else {
        providers::llm(&cfg)?.list_models().await?
    };
    // Prices that come with the model list are the provider's own, live.
    prices::add_listed(
        &app,
        models.iter().filter_map(|m| m.price.clone()).collect(),
    );
    Ok(models)
}

#[tauri::command]
pub fn get_pricing() -> PriceTable {
    (*PriceTable::current()).clone()
}

/// Reads prices again; `force` false allows a recent result to be reused.
#[tauri::command]
pub async fn refresh_pricing(app: AppHandle, force: bool) -> Res<PriceTable> {
    prices::refresh(&app, force).await
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

/// Async, like every command that starts or stops capture: synchronous
/// commands run on the main thread, and joining capture threads there
/// freezes every window.
#[tauri::command]
pub async fn audio_preview(app: AppHandle, window: tauri::WebviewWindow, on: bool) -> Res<()> {
    let owner = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || session::audio_preview(&app, &owner, on))
        .await
        .map_err(io_err)?
}

#[tauri::command]
pub async fn start_session(app: AppHandle) -> Res<()> {
    session::reported(&app, session::start(&app).await)
}

#[tauri::command]
pub async fn stop_session(app: AppHandle) -> Res<()> {
    session::stop(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn toggle_capture(app: AppHandle) -> Res<()> {
    session::reported(&app, session::toggle(&app).await)
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
pub fn reset_panel_position(app: AppHandle) {
    windows::reset_position(&app);
}

#[tauri::command]
pub fn change_panel(app: AppHandle, change: PanelChange) {
    windows::change_panel(&app, change);
}

/// How many screens are connected, so the panel offers "next screen" only
/// when there is one.
#[tauri::command]
pub fn panel_screens(app: AppHandle) -> u32 {
    app.available_monitors()
        .map(|m| m.len() as u32)
        .unwrap_or(1)
}

#[tauri::command]
pub fn set_always_on_top(app: AppHandle, on: bool) -> Res<()> {
    if let Some(w) = app.get_webview_window(windows::OVERLAY) {
        w.set_always_on_top(on).map_err(io_err)?;
    }
    Ok(())
}

/// Must be async: a synchronous command runs on the main thread inside the
/// webview's message handler, and creating a window there deadlocks on
/// Windows (wry#583). Every window would stop responding to moves and
/// clicks, and the app could not be quit.
#[tauri::command]
pub async fn open_window(app: AppHandle, view: String, section: Option<String>) -> Res<()> {
    const VIEWS: &[&str] = &["settings", "wizard", "practice", "review"];
    if !VIEWS.contains(&view.as_str()) {
        return Err(io_err("unknown view"));
    }
    windows::open_view(&app, &view, section.as_deref()).map_err(io_err)
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Res<()> {
    // Only web links and the system settings pages named in the error catalog.
    let allowed = url.starts_with("https://")
        || url.starts_with("ms-settings:")
        || url == "x-apple.systempreferences:com.apple.preference.universalaccess?SpokenContent";
    if !allowed {
        return Err(io_err("blocked URL"));
    }
    app.opener().open_url(url, None::<&str>).map_err(io_err)
}

#[tauri::command]
pub fn recent_logs(after: u64) -> Vec<LogLine> {
    crate::logging::recent(after)
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
    session::refresh_system_proxy();
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
                // Time the provider's lightest model when none is chosen.
                cfg.model = EndpointCatalog::builtin()
                    .probe_model(&target.provider)
                    .unwrap_or_default()
                    .to_string();
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
            let micros = PriceTable::current()
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
pub fn practice_sample(name: String) -> Res<PracticeScript> {
    practice::sample(&name)
}

#[tauri::command]
pub async fn list_voices() -> Vec<Voice> {
    tauri::async_runtime::spawn_blocking(|| {
        Speaker::new()
            .voices()
            .into_iter()
            .map(|v| Voice {
                id: v.id,
                name: v.name,
                lang: v.lang,
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn preview_voice(app: AppHandle, voice: String, lang: String) -> Res<()> {
    let rate = app.state::<AppState>().settings().practice.speech_rate;
    let sample = practice::voice_sample(&lang);
    tauri::async_runtime::spawn_blocking(move || Speaker::new().speak(sample, &lang, rate, &voice))
        .await
        .map_err(io_err)?
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
        "repeat" => practice::Control::Repeat,
        _ => return Err(io_err("unknown control")),
    };
    practice::control(&app, c);
    Ok(())
}

/// The question of each answer turn of the running practice.
#[tauri::command]
pub fn practice_outline(app: AppHandle) -> Vec<String> {
    practice::outline(&app)
}

/// Goes to answer turn `turn` (1-based) of the running practice.
#[tauri::command]
pub fn practice_jump(app: AppHandle, turn: u32) {
    practice::control(&app, practice::Control::Jump(turn));
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
        wipe_all_data,
        verify_provider,
        list_models,
        get_pricing,
        refresh_pricing,
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
        reset_panel_position,
        change_panel,
        panel_screens,
        open_window,
        open_url,
        hotkey_status,
        recent_logs,
        run_probe,
        probe_history,
        usage_summary,
        export_usage_csv,
        diagnostics_preview,
        export_diagnostics,
        import_material,
        practice_import,
        practice_sample,
        list_voices,
        preview_voice,
        practice_classify,
        practice_start,
        practice_control,
        practice_outline,
        practice_jump,
        practice_reports,
        practice_report,
        export_report,
    ]
}
