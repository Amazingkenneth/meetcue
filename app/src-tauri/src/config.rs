//! Loading and saving `settings.json`.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use meetcue_core::Settings;

/// Loads settings; a missing file gives defaults, a corrupt one is kept as
/// `settings.json.bad` and replaced by defaults.
pub fn load(path: &Path) -> Settings {
    match fs::read_to_string(path) {
        Ok(text) => match Settings::from_json(&text) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("settings file unreadable ({e}); using defaults");
                let _ = fs::rename(path, path.with_extension("json.bad"));
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}

/// Writes atomically (temporary file, then rename) so a crash never leaves
/// half a settings file.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    write_atomic(path, settings.to_json().as_bytes())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // One temporary file per write: settings are saved from the interface
    // and from the shell (panel position on exit), sometimes at once.
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}.{n}.tmp", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    // On Windows the rename fails while a scanner or a sync tool has the
    // target open; it is released within moments.
    let mut attempt = 0;
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < 4 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50 * attempt));
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(load(&path), Settings::default());
        let mut s = Settings::default();
        s.hints.role = "SRE".into();
        save(&path, &s).unwrap();
        assert_eq!(load(&path).hints.role, "SRE");
        fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), Settings::default());
        assert!(dir.path().join("settings.json.bad").exists());
    }
}
