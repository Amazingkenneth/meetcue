//! Config directory resolution.
//!
//! On Windows MeetCue runs portable: settings live in `config/` next to the
//! executable, which Scoop keeps across upgrades (`persist`). If that folder
//! cannot be written, settings fall back to `%APPDATA%\MeetCue` and the app
//! shows `MC-SYS-002` once. macOS and Linux use the standard per-user folders.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub portable: bool,
    /// Set when portable mode was wanted but the folder was not writable.
    pub fell_back: bool,
    /// The portable folder and the error that made it unusable.
    pub fallback_reason: Option<String>,
}

impl AppPaths {
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.config_dir.join("logs")
    }

    pub fn practice_dir(&self) -> PathBuf {
        self.config_dir.join("practice")
    }

    pub fn latency_history(&self) -> PathBuf {
        self.config_dir.join("latency-history.json")
    }

    /// The last price list downloaded, used until a newer one arrives.
    pub fn pricing_cache(&self) -> PathBuf {
        self.config_dir.join("pricing.json")
    }

    pub fn usage_ledger(&self) -> PathBuf {
        self.config_dir.join("usage.jsonl")
    }

    /// Whether the fallback notice still has to be shown. It is shown once
    /// per settings folder: the fallback is harmless, and a notice on every
    /// start reads like something keeps going wrong.
    pub fn fallback_notice_due(&self) -> bool {
        if !self.fell_back {
            return false;
        }
        let marker = self.config_dir.join(".portable-fallback-noticed");
        if marker.exists() {
            return false;
        }
        let _ = fs::create_dir_all(&self.config_dir);
        let _ = fs::write(&marker, self.fallback_reason.as_deref().unwrap_or_default());
        true
    }

    /// Creates the directory tree.
    pub fn ensure(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        fs::create_dir_all(self.logs_dir())?;
        fs::create_dir_all(self.practice_dir())?;
        Ok(())
    }
}

/// Whether a directory can be created and written to.
pub fn is_writable(dir: &Path) -> bool {
    write_error(dir).is_none()
}

/// Why a directory can't be created and written to, or `None` if it can.
///
/// Tries three times: right after sign-in or an update, antivirus scanners
/// and the search indexer briefly hold newly created files, and a single
/// failed probe used to move all settings to the fallback folder.
pub fn write_error(dir: &Path) -> Option<String> {
    let mut last = None;
    for attempt in 0..3 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(150));
        }
        match probe(dir) {
            Ok(()) => return None,
            Err(e) => last = Some(e.to_string()),
        }
    }
    last
}

fn probe(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    // A name of its own, so two copies starting at once (autostart and a
    // click on the shortcut) never trip over each other's probe file.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    let path = dir.join(format!(
        ".meetcue-write-test-{}-{nanos}",
        std::process::id()
    ));
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut f| f.write_all(b"ok"));
    let _ = fs::remove_file(&path);
    written
}

/// Pure resolution logic, testable with temporary directories.
pub fn resolve_with(
    exe_dir: Option<&Path>,
    user_config_dir: PathBuf,
    prefer_portable: bool,
) -> AppPaths {
    if prefer_portable {
        if let Some(exe_dir) = exe_dir {
            let portable = exe_dir.join("config");
            let Some(error) = write_error(&portable) else {
                return AppPaths {
                    config_dir: portable,
                    portable: true,
                    fell_back: false,
                    fallback_reason: None,
                };
            };
            return AppPaths {
                config_dir: user_config_dir,
                portable: false,
                fell_back: true,
                fallback_reason: Some(format!("{}: {error}", portable.display())),
            };
        }
    }
    AppPaths {
        config_dir: user_config_dir,
        portable: false,
        fell_back: false,
        fallback_reason: None,
    }
}

/// Resolves the paths for this process. `MEETCUE_CONFIG_DIR` overrides
/// everything (used by tests and the self-test).
pub fn resolve() -> AppPaths {
    if let Some(dir) = std::env::var_os("MEETCUE_CONFIG_DIR") {
        return AppPaths {
            config_dir: PathBuf::from(dir),
            portable: false,
            fell_back: false,
            fallback_reason: None,
        };
    }
    let user = ProjectDirs::from("", "", "MeetCue")
        .map(|p| {
            if cfg!(target_os = "linux") {
                // ~/.config/meetcue
                p.config_dir()
                    .parent()
                    .map(|d| d.join("meetcue"))
                    .unwrap_or_else(|| p.config_dir().to_path_buf())
            } else {
                p.config_dir().to_path_buf()
            }
        })
        .unwrap_or_else(|| PathBuf::from("config"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    resolve_with(exe_dir.as_deref(), user, cfg!(windows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_when_writable() {
        let exe = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        let p = resolve_with(Some(exe.path()), user.path().to_path_buf(), true);
        assert!(p.portable && !p.fell_back);
        assert_eq!(p.config_dir, exe.path().join("config"));
        p.ensure().unwrap();
        assert!(p.logs_dir().is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn falls_back_when_not_writable() {
        use std::os::unix::fs::PermissionsExt;
        let exe = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        fs::set_permissions(exe.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let p = resolve_with(Some(exe.path()), user.path().to_path_buf(), true);
        fs::set_permissions(exe.path(), fs::Permissions::from_mode(0o755)).unwrap();
        // Root can write anywhere; only assert the fallback when the probe failed.
        if !p.portable {
            assert!(p.fell_back);
            assert_eq!(p.config_dir, user.path());
            assert!(p.fallback_reason.is_some());
            // Shown once, not on every start.
            assert!(p.fallback_notice_due());
            assert!(!p.fallback_notice_due());
        }
    }

    #[test]
    fn probe_leaves_no_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_writable(dir.path()));
        assert!(is_writable(dir.path()));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn notice_only_when_fallen_back() {
        let user = tempfile::tempdir().unwrap();
        let p = resolve_with(None, user.path().to_path_buf(), true);
        assert!(!p.fallback_notice_due());
    }

    #[test]
    fn not_portable_off_windows() {
        let exe = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        let p = resolve_with(Some(exe.path()), user.path().to_path_buf(), false);
        assert!(!p.portable && !p.fell_back);
        assert_eq!(p.settings_file(), user.path().join("settings.json"));
    }
}
