//! Config directory resolution.
//!
//! On Windows MeetCue runs portable: settings live in `config/` next to the
//! executable, which Scoop keeps across upgrades (`persist`). If that folder
//! cannot be written, settings fall back to `%APPDATA%\MeetCue` and the app
//! shows `MC-SYS-002` once. macOS and Linux use the standard per-user folders.

use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub portable: bool,
    /// Set when portable mode was wanted but the folder was not writable.
    pub fell_back: bool,
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
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".meetcue-write-test");
    let ok = fs::write(&probe, b"ok").is_ok();
    let _ = fs::remove_file(&probe);
    ok
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
            if is_writable(&portable) {
                return AppPaths {
                    config_dir: portable,
                    portable: true,
                    fell_back: false,
                };
            }
            return AppPaths {
                config_dir: user_config_dir,
                portable: false,
                fell_back: true,
            };
        }
    }
    AppPaths {
        config_dir: user_config_dir,
        portable: false,
        fell_back: false,
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
        }
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
