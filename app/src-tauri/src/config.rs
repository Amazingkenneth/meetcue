//! Loading and saving `settings.json`.
//!
//! The text extracted from knowledge base files can be long, so it is kept
//! in `knowledge.json` next to the settings and written only when it
//! changes; `settings.json` keeps each file's name and path. Older settings
//! files with the text inside still load, and move it out on the next save.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use meetcue_core::settings::Material;
use meetcue_core::Settings;

/// Where the knowledge base text lives, next to the settings file.
pub fn knowledge_path(settings_path: &Path) -> std::path::PathBuf {
    settings_path.with_file_name("knowledge.json")
}

fn read_knowledge(path: &Path) -> Vec<Material> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Puts the saved text back into materials that were stored without it,
/// matching by path and name.
fn fill_knowledge(settings: &mut Settings, stored: Vec<Material>) {
    for m in settings
        .hints
        .materials
        .iter_mut()
        .filter(|m| m.text.is_empty())
    {
        if let Some(k) = stored.iter().find(|k| k.path == m.path && k.name == m.name) {
            m.text = k.text.clone();
        }
    }
}

/// Loads settings; a missing file gives defaults, a corrupt one is kept as
/// `settings.json.bad` and replaced by defaults.
pub fn load(path: &Path) -> Settings {
    match fs::read_to_string(path) {
        Ok(text) => match Settings::from_json(&text) {
            Ok(mut s) => {
                fill_knowledge(&mut s, read_knowledge(&knowledge_path(path)));
                s
            }
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
    let knowledge = knowledge_path(path);
    let json = serde_json::to_string(&settings.hints.materials).expect("serialize materials");
    let unchanged = fs::read_to_string(&knowledge).is_ok_and(|old| old == json);
    if !unchanged {
        write_atomic(&knowledge, json.as_bytes())?;
    }
    let mut slim = settings.clone();
    for m in &mut slim.hints.materials {
        m.text.clear();
    }
    write_atomic(path, slim.to_json().as_bytes())
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

    #[test]
    fn knowledge_text_is_kept_out_of_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings::default();
        s.hints.materials.push(Material {
            name: "cv.md".into(),
            path: "C:/cv.md".into(),
            text: "Ten years of networks.".into(),
        });
        save(&path, &s).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("cv.md") && !raw.contains("Ten years"));
        assert_eq!(load(&path), s);

        // Files written before the split keep their text.
        fs::remove_file(knowledge_path(&path)).unwrap();
        fs::write(&path, s.to_json()).unwrap();
        assert_eq!(load(&path), s);
    }
}
