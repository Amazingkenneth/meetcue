//! State shared by commands, the tray, hotkeys and the event forwarder.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, RwLock};

use meetcue_audio::AudioSource;
use meetcue_core::ipc::HotkeyState;
use meetcue_core::{SessionState, Settings};
use meetcue_pipeline::probe::ProbeReport;
use meetcue_pipeline::LocalHintBackend;
use meetcue_platform::credentials::CredentialStore;
use meetcue_platform::paths::AppPaths;

use crate::practice::PracticeRunner;

pub const DOCS_URL: &str = "https://amazingkenneth.github.io/meetcue/";

pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub credentials: Arc<dyn CredentialStore>,
    pub backend: Arc<LocalHintBackend>,
    pub audio: Mutex<Option<Box<dyn AudioSource>>>,
    pub preview: Mutex<Preview>,
    pub session: Mutex<SessionState>,
    pub demo: bool,
    pub self_test: bool,
    pub system_locale: String,
    pub probes: Mutex<Vec<ProbeReport>>,
    pub hotkeys: Mutex<Vec<HotkeyState>>,
    pub practice: Mutex<Option<PracticeRunner>>,
}

/// Capture for the level meters while no session runs.
#[derive(Default)]
pub struct Preview {
    /// Labels of the windows that show level meters: the panel while it's
    /// visible, and the settings window while its level test is on.
    pub owners: BTreeSet<String>,
    pub source: Option<Box<dyn AudioSource>>,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Resolved interface language (`en` or `zh-CN`).
    pub fn language(&self) -> &'static str {
        self.settings
            .read()
            .unwrap()
            .general
            .ui_language
            .resolve(&self.system_locale)
    }

    pub fn session_state(&self) -> SessionState {
        *self.session.lock().unwrap()
    }
}
