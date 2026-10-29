//! "Export diagnostics": a zip with recent logs, the settings (without
//! secrets, transcripts or briefing material), latency history and system
//! information. The user sees the list of contents before exporting.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use meetcue_core::error::redact;
use meetcue_core::{codes, MeetCueError, Settings};

use crate::state::AppState;

/// Log target for transcripts written with debug logging on. Lines with this
/// target stay on the user's computer and are left out of the export.
pub const TRANSCRIPT_TARGET: &str = "meetcue_transcript";

fn strip_transcripts(log: &str) -> String {
    let marker = format!(" {TRANSCRIPT_TARGET}:");
    log.lines()
        .filter(|l| !l.contains(&marker))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Log files from the last three days.
fn recent_logs(dir: &Path) -> Vec<PathBuf> {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 24 * 3600);
    let mut logs: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .map(|t| t >= cutoff)
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("meetcue"))
        })
        .collect();
    logs.sort();
    logs
}

/// Settings safe to share: material text and role removed, proxy credentials masked.
pub fn shareable_settings(s: &Settings) -> String {
    let mut s = s.clone();
    for m in &mut s.hints.materials {
        m.text = format!("({} characters removed)", m.text.chars().count());
    }
    if !s.hints.role.is_empty() {
        s.hints.role = "(removed)".into();
    }
    // Custom prompts can quote the user's background; keep only whether they exist.
    for p in [
        &mut s.hints.prompts.hints_system,
        &mut s.hints.prompts.hints_user,
    ] {
        if !p.is_empty() {
            *p = format!("(custom, {} characters removed)", p.chars().count());
        }
    }
    if let Some(p) = &s.providers.proxy {
        s.providers.proxy = Some(mask_userinfo(p));
    }
    for u in s.providers.base_urls.values_mut() {
        *u = mask_userinfo(u);
    }
    redact(&s.to_json())
}

fn mask_userinfo(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(scheme), Some(at)) if at > scheme => {
            format!("{}***{}", &url[..scheme + 3], &url[at..])
        }
        _ => url.to_string(),
    }
}

fn system_info(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let mut out = format!(
        "MeetCue {}\nOS: {} {}\nLocale: {}\nWebView: {}\nPortable: {}\n",
        meetcue_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        state.system_locale,
        tauri::webview_version().unwrap_or_else(|e| format!("unavailable ({e})")),
        state.paths.portable,
    );
    let source = meetcue_audio::platform_source();
    for (label, list) in [
        ("Microphones", source.list_microphones()),
        ("Outputs", source.list_outputs()),
    ] {
        out.push_str(&format!("{label}:\n"));
        for d in list.unwrap_or_default() {
            out.push_str(&format!(
                "  - {}{}{}\n",
                d.name,
                if d.is_default { " (default)" } else { "" },
                if d.is_bluetooth { " (bluetooth)" } else { "" }
            ));
        }
    }
    out
}

pub fn preview(app: &AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    let mut items: Vec<String> = recent_logs(&state.paths.logs_dir())
        .iter()
        .filter_map(|p| {
            p.file_name()
                .map(|n| format!("logs/{}", n.to_string_lossy()))
        })
        .collect();
    items.push("settings.json (no API keys, role or briefing text)".into());
    if state.paths.latency_history().exists() {
        items.push("latency-history.json".into());
    }
    items.push("system.txt (versions, audio device names)".into());
    items
}

pub fn write_zip(app: &AppHandle, target: &Path) -> Result<(), MeetCueError> {
    let state = app.state::<AppState>();
    let err = |e: &dyn std::fmt::Display| MeetCueError::new(codes::DIAG_EXPORT, e.to_string());
    let file = fs::File::create(target).map_err(|e| err(&e))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for log in recent_logs(&state.paths.logs_dir()) {
        let name = format!(
            "logs/{}",
            log.file_name().unwrap_or_default().to_string_lossy()
        );
        let text = fs::read_to_string(&log).unwrap_or_default();
        zip.start_file(name, opts).map_err(|e| err(&e))?;
        // Logs are redacted when written; redact again in case of older lines.
        zip.write_all(redact(&strip_transcripts(&text)).as_bytes())
            .map_err(|e| err(&e))?;
    }
    zip.start_file("settings.json", opts).map_err(|e| err(&e))?;
    zip.write_all(shareable_settings(&state.settings()).as_bytes())
        .map_err(|e| err(&e))?;
    if let Ok(h) = fs::read_to_string(state.paths.latency_history()) {
        zip.start_file("latency-history.json", opts)
            .map_err(|e| err(&e))?;
        zip.write_all(h.as_bytes()).map_err(|e| err(&e))?;
    }
    zip.start_file("system.txt", opts).map_err(|e| err(&e))?;
    zip.write_all(system_info(app).as_bytes())
        .map_err(|e| err(&e))?;
    zip.finish().map_err(|e| err(&e))?;
    Ok(())
}

/// Asks where to save, then writes the zip. Returns the saved path.
pub async fn export_interactive(app: &AppHandle) -> Result<Option<String>, MeetCueError> {
    let name = format!(
        "meetcue-diagnostics-{}.zip",
        chrono::Local::now().format("%Y%m%d-%H%M")
    );
    let dialog = app
        .dialog()
        .file()
        .set_file_name(&name)
        .add_filter("Zip", &["zip"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| MeetCueError::new(codes::DIAG_EXPORT, e.to_string()))?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    write_zip(app, &path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use meetcue_core::settings::Material;

    #[test]
    fn export_leaves_out_transcripts() {
        let log = "2026-10-23T10:00:00Z  INFO meetcue::session: started\n\
                   2026-10-23T10:00:01Z DEBUG meetcue_transcript: channel=\"remote\" what is SDN\n\
                   2026-10-23T10:00:02Z  INFO meetcue::session: stopped\n";
        let out = strip_transcripts(log);
        assert!(!out.contains("what is SDN"));
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn shareable_settings_drop_private_text() {
        let mut s = Settings::default();
        s.hints.role = "Senior engineer at a secret project".into();
        s.hints.materials.push(Material {
            name: "cv.txt".into(),
            path: "cv.txt".into(),
            text: "private résumé text".into(),
        });
        s.providers.proxy = Some("http://alice:hunter2@proxy.local:3128".into());
        let out = shareable_settings(&s);
        assert!(!out.contains("private résumé"));
        assert!(!out.contains("secret project"));
        assert!(!out.contains("hunter2"));
        assert!(out.contains("proxy.local:3128"));
    }
}
