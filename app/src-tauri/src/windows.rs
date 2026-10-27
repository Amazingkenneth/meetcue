//! Window management: the overlay panel (full, mini, hidden) and the normal
//! settings, wizard, practice and review windows.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use meetcue_core::ipc::{AppEvent, PanelChange};
use meetcue_core::settings::{
    OverlaySettings, PanelHeight, PanelMode, PanelSpot, OPACITY_RANGE, TEXT_SCALE_RANGE,
};
use meetcue_platform::window::Rect;

use crate::state::AppState;
use crate::{config, APP_EVENT};

pub const OVERLAY: &str = "overlay";
const WIDTH: f64 = 440.0;
const MIN_WIDTH: f64 = 300.0;
const MIN_HEIGHT: f64 = 160.0;
const MINI_HEIGHT: f64 = 200.0;
const MARGIN: f64 = 20.0;
/// Gap between a snapped panel and the screen edge, in logical pixels.
const SNAP_MARGIN: f64 = 12.0;
/// How long the panel stays out of sight when hidden for a moment.
const PEEK: Duration = Duration::from_secs(6);
/// Logical pixels per press of a move shortcut, and per frame while held.
const NUDGE_STEP: f64 = 10.0;
const GLIDE_STEP: f64 = 6.0;
const GLIDE_DELAY: Duration = Duration::from_millis(300);
const GLIDE_FRAME: Duration = Duration::from_millis(16);
/// A missed key release must not leave the panel drifting.
const GLIDE_MAX: Duration = Duration::from_secs(15);
/// How much of the panel stays on screen when it is moved down.
const KEEP_VISIBLE: f64 = 120.0;

fn default_full_height(win: &WebviewWindow) -> f64 {
    win.current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten())
        .map(|m| m.size().height as f64 / m.scale_factor() - 80.0)
        .unwrap_or(800.0)
        .max(400.0)
}

/// The logical size the panel has in `mode`.
fn panel_size(win: &WebviewWindow, mode: PanelMode, o: &OverlaySettings) -> (f64, f64) {
    let w = o.width.unwrap_or(WIDTH).max(MIN_WIDTH);
    let h = match mode {
        PanelMode::Mini => MINI_HEIGHT,
        _ => o
            .height
            .unwrap_or_else(|| default_full_height(win))
            .max(MIN_HEIGHT),
    };
    (w, h)
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
    // A double click (or a touchpad double tap) on the panel must never
    // blow it up to cover the whole screen.
    .maximizable(false)
    .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
    // Created unfocusable and unfocused (not changed after creation), so
    // even the first frame does not steal focus.
    .focused(false)
    .focusable(false)
    .visible(false)
    .content_protected(settings.overlay.hide_in_screen_share)
    .inner_size(settings.overlay.width.unwrap_or(WIDTH), 700.0)
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
    apply_size(&win, mode, &settings.overlay);
    place(&win, settings.overlay.x, settings.overlay.y);
    if !state.self_test {
        let _ = win.show();
        show_levels(app, true);
    }

    let handle = app.clone();
    win.on_window_event(move |e| match e {
        tauri::WindowEvent::Moved(pos) => remember_position(&handle, *pos),
        tauri::WindowEvent::Resized(size) => remember_size(&handle, *size),
        _ => {}
    });
    Ok(win)
}

fn apply_size(win: &WebviewWindow, mode: PanelMode, o: &OverlaySettings) {
    let (w, h) = panel_size(win, mode, o);
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
}

/// Keeps a size the user dragged the panel edges to. Sizes the app set
/// itself match what `panel_size` gives and are not stored, so the default
/// full height keeps following the screen.
fn remember_size(app: &AppHandle, size: tauri::PhysicalSize<u32>) {
    let Some(win) = app.get_webview_window(OVERLAY) else {
        return;
    };
    if size.width == 0 || size.height == 0 || !win.is_visible().unwrap_or(false) {
        return;
    }
    let scale = win.scale_factor().unwrap_or(1.0);
    let (w, h) = (size.width as f64 / scale, size.height as f64 / scale);
    let state = app.state::<AppState>();
    let mut s = state.settings.write().unwrap();
    let mode = s.overlay.mode;
    let (ew, eh) = panel_size(&win, mode, &s.overlay);
    if (w - ew).abs() > 2.0 {
        s.overlay.width = Some(w.round());
    }
    if mode == PanelMode::Full && (h - eh).abs() > 2.0 {
        s.overlay.height = Some(h.round());
    }
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
    // Stored first: the resize below reports back, and must be judged
    // against the new layout's size.
    {
        let state = app.state::<AppState>();
        state.settings.write().unwrap().overlay.mode = mode;
    }
    match mode {
        PanelMode::Hidden => {
            let _ = win.hide();
        }
        m => {
            let overlay = app.state::<AppState>().settings().overlay;
            apply_size(&win, m, &overlay);
            keep_on_screen(&win);
            PEEKING.store(false, Ordering::SeqCst);
            let _ = win.show();
        }
    }
    show_levels(app, mode != PanelMode::Hidden);
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

#[derive(Clone, Copy)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// One counter per direction: a press or release bumps it, which ends the
/// glide started by the previous press. Separate counters let two arrows
/// held together move the panel diagonally.
static HOLDS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// Handles a move shortcut: a press moves the panel a little, holding it
/// keeps the panel moving until the keys are released.
pub fn move_key(app: &AppHandle, dir: Direction, pressed: bool) {
    let hold = HOLDS[dir as usize].fetch_add(1, Ordering::SeqCst) + 1;
    if !pressed {
        persist(app);
        return;
    }
    if !nudge(app, dir, NUDGE_STEP) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(GLIDE_DELAY).await;
        let start = std::time::Instant::now();
        while HOLDS[dir as usize].load(Ordering::SeqCst) == hold
            && start.elapsed() < GLIDE_MAX
            && nudge(&app, dir, GLIDE_STEP)
        {
            tokio::time::sleep(GLIDE_FRAME).await;
        }
    });
}

/// Moves the visible panel by `step` logical pixels, keeping it on screen.
/// Returns false when it could not move (hidden, or already at the edge).
fn nudge(app: &AppHandle, dir: Direction, step: f64) -> bool {
    use meetcue_platform::window::{nudge_target, Rect};
    let Some(win) = app.get_webview_window(OVERLAY) else {
        return false;
    };
    if !win.is_visible().unwrap_or(false) {
        return false;
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return false;
    };
    let scale = win.scale_factor().unwrap_or(1.0);
    let d = (step * scale).round() as i32;
    let (dx, dy) = match dir {
        Direction::Up => (0, -d),
        Direction::Down => (0, d),
        Direction::Left => (-d, 0),
        Direction::Right => (d, 0),
    };
    let screens: Vec<Rect> = win
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| Rect {
            x: m.position().x,
            y: m.position().y,
            w: m.size().width as i32,
            h: m.size().height as i32,
        })
        .collect();
    let current = Rect {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    };
    let keep = (KEEP_VISIBLE * scale) as i32;
    let (x, y) = nudge_target(&screens, current, dx, dy, keep);
    if (x, y) == (pos.x, pos.y) {
        return false;
    }
    win.set_position(PhysicalPosition::new(x, y)).is_ok()
}

/// The screens' free areas (without the taskbar) and the panel's rectangle,
/// in physical pixels.
fn layout(win: &WebviewWindow) -> Option<(Vec<Rect>, Rect)> {
    let (pos, size) = (win.outer_position().ok()?, win.outer_size().ok()?);
    let mut areas: Vec<Rect> = win
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let a = m.work_area();
            Rect {
                x: a.position.x,
                y: a.position.y,
                w: a.size.width as i32,
                h: a.size.height as i32,
            }
        })
        .collect();
    // Left to right, so "next screen" walks across the desk.
    areas.sort_by_key(|a| (a.x, a.y));
    let panel = Rect {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    };
    Some((areas, panel))
}

/// Pulls the panel back inside the free area of its screen after it grew.
fn keep_on_screen(win: &WebviewWindow) {
    use meetcue_platform::window::screen_of;
    let Some((areas, p)) = layout(win) else {
        return;
    };
    let Some(a) = screen_of(&areas, p).map(|i| areas[i]) else {
        return;
    };
    let x = p.x.min(a.x + a.w - p.w).max(a.x);
    let y = p.y.min(a.y + a.h - p.h).max(a.y);
    if (x, y) != (p.x, p.y) {
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

/// Saves the settings and tells every window about them.
fn settings_changed(app: &AppHandle) {
    persist(app);
    let settings = app.state::<AppState>().settings();
    let _ = app.emit(
        APP_EVENT,
        AppEvent::SettingsChanged {
            settings: Box::new(settings),
        },
    );
}

/// Bumped by every peek, so an older peek's timer doesn't bring the panel
/// back early.
static PEEK_ROUND: AtomicU64 = AtomicU64::new(0);
static PEEKING: AtomicBool = AtomicBool::new(false);

/// Hides the visible panel for a few seconds so the user can click what is
/// behind it; a second peek brings it back at once.
fn peek(app: &AppHandle) {
    let Some(win) = app.get_webview_window(OVERLAY) else {
        return;
    };
    let round = PEEK_ROUND.fetch_add(1, Ordering::SeqCst) + 1;
    if PEEKING.swap(false, Ordering::SeqCst) {
        let _ = win.show();
        return;
    }
    if !win.is_visible().unwrap_or(false) {
        return;
    }
    PEEKING.store(true, Ordering::SeqCst);
    let _ = win.hide();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PEEK).await;
        if PEEK_ROUND.load(Ordering::SeqCst) == round && PEEKING.swap(false, Ordering::SeqCst) {
            let _ = win.show();
        }
    });
}

/// Applies a placement, size or visibility change to the panel.
pub fn change_panel(app: &AppHandle, change: PanelChange) {
    use meetcue_platform::window::{same_spot_on, screen_of, snap_target};
    let Some(win) = app.get_webview_window(OVERLAY) else {
        return;
    };
    let state = app.state::<AppState>();
    let scale = win.scale_factor().unwrap_or(1.0);
    match change {
        PanelChange::Snap { place } => {
            let Some((areas, p)) = layout(&win) else {
                return;
            };
            let Some(area) = screen_of(&areas, p).map(|i| areas[i]) else {
                return;
            };
            let margin = (SNAP_MARGIN * scale).round() as i32;
            let (x, y) = snap_target(area, p.w, p.h, place, margin);
            let _ = win.set_position(PhysicalPosition::new(x, y));
            persist(app);
        }
        PanelChange::NextScreen => {
            let Some((areas, p)) = layout(&win) else {
                return;
            };
            let Some(i) = screen_of(&areas, p) else {
                return;
            };
            if areas.len() < 2 {
                return;
            }
            let to = areas[(i + 1) % areas.len()];
            let (x, y) = same_spot_on(areas[i], to, p);
            let _ = win.set_position(PhysicalPosition::new(x, y));
            keep_on_screen(&win);
            persist(app);
        }
        PanelChange::Width { width } => {
            state.settings.write().unwrap().overlay.width = Some(width.logical());
            resize_in_place(app, &win, None);
        }
        PanelChange::Height { height } => {
            let free = layout(&win)
                .and_then(|(areas, p)| screen_of(&areas, p).map(|i| areas[i].h as f64 / scale));
            let h = match (height, free) {
                (PanelHeight::Tall, _) | (_, None) => None,
                (PanelHeight::Half, Some(f)) => Some((f / 2.0).round()),
                (PanelHeight::Third, Some(f)) => Some((f / 3.0).round()),
            };
            state.settings.write().unwrap().overlay.height = h;
            // A height only means something for the full panel.
            resize_in_place(app, &win, Some(PanelMode::Full));
        }
        PanelChange::GoSpot { slot } => {
            let spot = state
                .settings()
                .overlay
                .spots
                .get(slot as usize)
                .cloned()
                .flatten();
            match spot {
                Some(spot) => {
                    {
                        let mut s = state.settings.write().unwrap();
                        s.overlay.width = Some(spot.width);
                        if spot.mode == PanelMode::Full {
                            s.overlay.height = Some(spot.height);
                        }
                    }
                    set_mode(app, spot.mode);
                    place(&win, Some(spot.x), Some(spot.y));
                    keep_on_screen(&win);
                    persist(app);
                    let _ = app.emit(APP_EVENT, AppEvent::PanelSpot { slot, saved: false });
                }
                None => change_panel(app, PanelChange::SaveSpot { slot }),
            }
        }
        PanelChange::SaveSpot { slot } => {
            let (Ok(pos), Ok(size)) = (win.outer_position(), win.inner_size()) else {
                return;
            };
            {
                let mut s = state.settings.write().unwrap();
                let mode = match s.overlay.mode {
                    PanelMode::Hidden => PanelMode::Full,
                    m => m,
                };
                if let Some(entry) = s.overlay.spots.get_mut(slot as usize) {
                    *entry = Some(PanelSpot {
                        x: pos.x,
                        y: pos.y,
                        width: (size.width as f64 / scale).round(),
                        height: (size.height as f64 / scale).round(),
                        mode,
                    });
                }
            }
            settings_changed(app);
            let _ = app.emit(APP_EVENT, AppEvent::PanelSpot { slot, saved: true });
        }
        PanelChange::ClearSpot { slot } => {
            if let Some(entry) = state
                .settings
                .write()
                .unwrap()
                .overlay
                .spots
                .get_mut(slot as usize)
            {
                *entry = None;
            }
            settings_changed(app);
        }
        PanelChange::Peek => peek(app),
        PanelChange::ClickThrough { on } => {
            state.settings.write().unwrap().overlay.click_through = on;
            apply_overlay_settings(app);
            settings_changed(app);
            crate::tray::refresh(app);
        }
        PanelChange::Opacity { value } => {
            let (lo, hi) = OPACITY_RANGE;
            state.settings.write().unwrap().overlay.opacity = value.clamp(lo, hi);
            settings_changed(app);
        }
        PanelChange::TextScale { value } => {
            let (lo, hi) = TEXT_SCALE_RANGE;
            state.settings.write().unwrap().overlay.text_scale = value.clamp(lo, hi);
            settings_changed(app);
        }
    }
}

/// Applies the stored size, keeping the panel's top left corner where it is
/// unless that would push it off screen.
fn resize_in_place(app: &AppHandle, win: &WebviewWindow, mode: Option<PanelMode>) {
    let current = app.state::<AppState>().settings().overlay.mode;
    match mode {
        Some(m) if m != current => set_mode(app, m),
        _ => {
            let overlay = app.state::<AppState>().settings().overlay;
            apply_size(win, current, &overlay);
            keep_on_screen(win);
            settings_changed(app);
        }
    }
}

/// Toggles click-through, from the shortcut or the tray.
pub fn toggle_click_through(app: &AppHandle) {
    let on = !app.state::<AppState>().settings().overlay.click_through;
    change_panel(app, PanelChange::ClickThrough { on });
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
