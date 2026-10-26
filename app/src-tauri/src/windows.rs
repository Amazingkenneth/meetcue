//! Window management: the overlay panel (full, mini, hidden) and the normal
//! settings, wizard, practice and review windows.

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use meetcue_core::ipc::AppEvent;
use meetcue_core::settings::PanelMode;

use crate::state::AppState;
use crate::{config, APP_EVENT};

pub const OVERLAY: &str = "overlay";
const WIDTH: f64 = 440.0;
const MINI_HEIGHT: f64 = 200.0;
const MARGIN: f64 = 20.0;

fn full_height(win: &WebviewWindow) -> f64 {
    win.current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten())
        .map(|m| m.size().height as f64 / m.scale_factor() - 80.0)
        .unwrap_or(800.0)
        .max(400.0)
}

/// Creates the overlay without ever taking focus from the meeting app.
pub fn create_overlay(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let builder = WebviewWindowBuilder::new(
        app,
        OVERLAY,
        WebviewUrl::App("index.html?view=overlay".into()),
    )
    .title("MeetCue")
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .resizable(true)
    // Created unfocusable and unfocused (not changed after creation), so
    // even the first frame does not steal focus.
    .focused(false)
    .focusable(false)
    .visible(false)
    .content_protected(settings.overlay.hide_in_screen_share)
    .inner_size(WIDTH, 700.0)
    .transparent(true);
    let win = builder.build()?;

    #[cfg(windows)]
    if let Ok(hwnd) = win.hwnd() {
        let raw = hwnd.0 as isize;
        let _ = meetcue_platform::window::make_non_activating(raw);
        let _ = meetcue_platform::window::exclude_from_capture(
            raw,
            settings.overlay.hide_in_screen_share,
        );
    }
    let _ = win.set_ignore_cursor_events(settings.overlay.click_through);

    let mode = match settings.overlay.mode {
        PanelMode::Hidden => PanelMode::Full,
        m => m,
    };
    apply_size(&win, mode);
    place(&win, settings.overlay.x, settings.overlay.y);
    if !state.self_test {
        let _ = win.show();
        show_levels(app, true);
    }

    let handle = app.clone();
    win.on_window_event(move |e| {
        if let tauri::WindowEvent::Moved(pos) = e {
            remember_position(&handle, *pos);
        }
    });
    Ok(win)
}

fn apply_size(win: &WebviewWindow, mode: PanelMode) {
    let h = match mode {
        PanelMode::Mini => MINI_HEIGHT,
        _ => full_height(win),
    };
    let _ = win.set_size(tauri::LogicalSize::new(WIDTH, h));
}

/// Puts the panel at the saved position if that is on a connected screen,
/// otherwise at the top right of the primary screen.
pub fn place(win: &WebviewWindow, x: Option<i32>, y: Option<i32>) {
    let monitors = win.available_monitors().unwrap_or_default();
    if let (Some(x), Some(y)) = (x, y) {
        let visible = monitors.iter().any(|m| {
            let p = m.position();
            let s = m.size();
            x >= p.x - 40
                && y >= p.y - 10
                && x < p.x + s.width as i32 - 80
                && y < p.y + s.height as i32 - 80
        });
        if visible {
            let _ = win.set_position(PhysicalPosition::new(x, y));
            return;
        }
    }
    if let Some(m) = win.primary_monitor().ok().flatten() {
        let scale = m.scale_factor();
        let x = m.position().x as f64 + m.size().width as f64 - (WIDTH + MARGIN) * scale;
        let y = m.position().y as f64 + 40.0 * scale;
        let _ = win.set_position(PhysicalPosition::new(x as i32, y as i32));
    }
}

fn remember_position(app: &AppHandle, pos: PhysicalPosition<i32>) {
    let state = app.state::<AppState>();
    let mut s = state.settings.write().unwrap();
    s.overlay.x = Some(pos.x);
    s.overlay.y = Some(pos.y);
    // Saved on mode changes and on exit to avoid writing on every pixel.
}

pub fn persist(app: &AppHandle) {
    let state = app.state::<AppState>();
    let s = state.settings();
    if let Err(e) = config::save(&state.paths.settings_file(), &s) {
        tracing::warn!("could not save settings: {e}");
    }
}

pub fn set_mode(app: &AppHandle, mode: PanelMode) {
    let Some(win) = app.get_webview_window(OVERLAY) else {
        return;
    };
    match mode {
        PanelMode::Hidden => {
            let _ = win.hide();
        }
        m => {
            apply_size(&win, m);
            let _ = win.show();
        }
    }
    show_levels(app, mode != PanelMode::Hidden);
    {
        let state = app.state::<AppState>();
        state.settings.write().unwrap().overlay.mode = mode;
    }
    persist(app);
    let _ = app.emit(APP_EVENT, AppEvent::PanelMode { mode });
    // Keeps the layout choice in the settings window in step.
    let settings = app.state::<AppState>().settings();
    let _ = app.emit(
        APP_EVENT,
        AppEvent::SettingsChanged {
            settings: Box::new(settings),
        },
    );
}

/// The visible panel shows live microphone and system sound levels even
/// before a session starts; a hidden one doesn't keep the microphone open.
fn show_levels(app: &AppHandle, on: bool) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = crate::session::audio_preview(&app, OVERLAY, on) {
            tracing::warn!(code = %e.code, "panel level meters unavailable: {}", e.detail);
        }
    });
}

pub fn cycle_mode(app: &AppHandle) {
    let state = app.state::<AppState>();
    let current = state.settings().overlay.mode;
    let visible = app
        .get_webview_window(OVERLAY)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    let next = match (current, visible) {
        (_, false) | (PanelMode::Hidden, _) => PanelMode::Full,
        (PanelMode::Full, true) => PanelMode::Mini,
        (PanelMode::Mini, true) => PanelMode::Hidden,
    };
    set_mode(app, next);
}

pub fn reset_position(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(OVERLAY) {
        place(&win, None, None);
        let _ = win.show();
        show_levels(app, true);
    }
}

/// Applies overlay settings that can change while running.
pub fn apply_overlay_settings(app: &AppHandle) {
    let state = app.state::<AppState>();
    let s = state.settings();
    if let Some(win) = app.get_webview_window(OVERLAY) {
        let _ = win.set_ignore_cursor_events(s.overlay.click_through);
        let _ = win.set_content_protected(s.overlay.hide_in_screen_share);
        #[cfg(windows)]
        if let Ok(hwnd) = win.hwnd() {
            let _ = meetcue_platform::window::exclude_from_capture(
                hwnd.0 as isize,
                s.overlay.hide_in_screen_share,
            );
        }
    }
}

/// Opens (or focuses) a normal window for a view, optionally at a section.
pub fn open_view(app: &AppHandle, view: &str, section: Option<&str>) -> tauri::Result<()> {
    let label = view.to_string();
    let url = match section {
        Some(s) => format!("index.html?view={view}#{s}"),
        None => format!("index.html?view={view}"),
    };
    if let Some(win) = app.get_webview_window(&label) {
        if section.is_some() {
            let _ = win.eval(format!("window.location.replace({:?})", format!("/{url}")));
        }
        win.show()?;
        win.set_focus()?;
        return Ok(());
    }
    let (w, h) = match view {
        "wizard" => (900.0, 760.0),
        "practice" => (820.0, 760.0),
        "review" => (900.0, 820.0),
        _ => (1000.0, 760.0),
    };
    let lang = app.state::<AppState>().language();
    let title = crate::i18n::t(lang, &format!("gallery.view.{view}"));
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title(format!("{title} · MeetCue"))
        .inner_size(w, h)
        .min_inner_size(560.0, 480.0)
        .center()
        .build()?;
    Ok(())
}
