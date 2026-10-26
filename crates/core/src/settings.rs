//! User settings, stored as `settings.json` in the config directory.
//!
//! API keys are never part of this struct; they live in the system
//! credential store. Every field has a default so older files keep loading.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::pricing::{CostAssumptions, PriceEntry};

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Settings {
    pub version: u32,
    pub general: GeneralSettings,
    pub audio: AudioSettings,
    pub providers: ProviderSettings,
    pub hints: HintSettings,
    pub hotkeys: Hotkeys,
    pub overlay: OverlaySettings,
    pub practice: PracticeSettings,
    pub diagnostics: DiagnosticsSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            general: Default::default(),
            audio: Default::default(),
            providers: Default::default(),
            hints: Default::default(),
            hotkeys: Default::default(),
            overlay: Default::default(),
            practice: Default::default(),
            diagnostics: Default::default(),
        }
    }
}

impl Settings {
    /// Parses a settings file, falling back to defaults for missing fields.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        let mut s: Settings = serde_json::from_str(json)?;
        s.version = SETTINGS_VERSION;
        // Older files kept one base URL, for the OpenAI-compatible provider only.
        if let Some(url) = s.providers.openai_base_url.take() {
            s.providers
                .base_urls
                .entry("openai-compatible".into())
                .or_insert(url);
        }
        Ok(s)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serialize")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum UiLanguage {
    /// Follow the operating system: Chinese if it starts with `zh`, otherwise English.
    System,
    En,
    #[serde(rename = "zh-CN")]
    #[ts(rename = "zh-CN")]
    ZhCn,
}

impl UiLanguage {
    /// Resolves to a concrete locale tag (`en` or `zh-CN`).
    pub fn resolve(self, system_locale: &str) -> &'static str {
        match self {
            UiLanguage::En => "en",
            UiLanguage::ZhCn => "zh-CN",
            UiLanguage::System => {
                if system_locale.to_ascii_lowercase().starts_with("zh") {
                    "zh-CN"
                } else {
                    "en"
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HintLanguage {
    /// Use the dominant language of the question.
    FollowQuestion,
    En,
    #[serde(rename = "zh-CN")]
    #[ts(rename = "zh-CN")]
    ZhCn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GeneralSettings {
    pub ui_language: UiLanguage,
    pub hint_language: HintLanguage,
    pub accent: String,
    pub autostart: bool,
    pub wizard_completed: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            ui_language: UiLanguage::System,
            hint_language: HintLanguage::FollowQuestion,
            accent: "#F5B841".into(),
            autostart: false,
            wizard_completed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RemoteSource {
    /// Everything played through the default output device.
    AllSystemSound,
    /// Only the meeting application (planned, not available yet).
    MeetingAppOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AudioSettings {
    /// Microphone device id; `None` uses the default device.
    pub mic_device: Option<String>,
    pub remote_source: RemoteSource,
    /// Energy threshold for voice activity, in dBFS.
    pub vad_threshold_db: f32,
    /// Silence that ends a sentence, in milliseconds.
    pub endpoint_silence_ms: u32,
    /// Stop interrupting practice playback when the user starts speaking.
    pub stop_tts_on_speech: bool,
    /// Languages spoken in meetings, as ISO codes (`zh`, `en`).
    pub languages: Vec<String>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            mic_device: None,
            remote_source: RemoteSource::AllSystemSound,
            vad_threshold_db: -45.0,
            endpoint_silence_ms: 700,
            stop_tts_on_speech: true,
            languages: vec!["zh".into(), "en".into()],
        }
    }
}

/// Endpoint choice for a provider. `Auto` uses the fastest endpoint measured
/// by the latency probe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", content = "host", rename_all = "camelCase")]
#[ts(export)]
pub enum EndpointChoice {
    #[default]
    Auto,
    Host(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ModelChoice {
    /// Provider id such as `soniox`, `deepgram`, `anthropic`, `openai-compatible`.
    pub provider: Option<String>,
    pub model: Option<String>,
    pub endpoint: EndpointChoice,
}

impl ModelChoice {
    pub fn is_set(&self) -> bool {
        self.provider.is_some() && self.model.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BudgetAction {
    NotifyOnly,
    PauseHints,
    EndSession,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct BudgetSettings {
    pub session_limit_usd: Option<f64>,
    pub monthly_limit_usd: Option<f64>,
    pub on_limit: BudgetAction,
}

impl Default for BudgetSettings {
    fn default() -> Self {
        Self {
            session_limit_usd: None,
            monthly_limit_usd: None,
            on_limit: BudgetAction::NotifyOnly,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ProviderSettings {
    pub asr: ModelChoice,
    pub detect: ModelChoice,
    pub hints: ModelChoice,
    /// Base URL per provider id, e.g. `https://host/v1` for `openai-compatible`.
    /// A provider without an entry uses its own default address.
    pub base_urls: BTreeMap<String, String>,
    /// Read from older files and moved into `base_urls`; never written.
    #[serde(skip_serializing)]
    #[ts(skip)]
    pub openai_base_url: Option<String>,
    /// Explicit HTTPS proxy; when empty the system proxy settings are used.
    pub proxy: Option<String>,
    pub pricing_overrides: Vec<PriceEntry>,
    pub estimate: CostAssumptions,
    pub budget: BudgetSettings,
}

impl ProviderSettings {
    /// The user's base URL for a provider, if they set one.
    pub fn base_url(&self, provider: &str) -> Option<String> {
        self.base_urls
            .get(provider)
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Material {
    pub name: String,
    pub path: String,
    /// Extracted plain text (kept locally; only a summary is sent to the model).
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct HintSettings {
    pub role: String,
    pub materials: Vec<Material>,
    pub only_directed_to_me: bool,
    pub queue_interruptions: bool,
    pub track_coverage: bool,
    pub early_trigger: bool,
}

impl Default for HintSettings {
    fn default() -> Self {
        Self {
            role: String::new(),
            materials: Vec::new(),
            only_directed_to_me: false,
            queue_interruptions: true,
            track_coverage: true,
            early_trigger: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Hotkeys {
    pub toggle_capture: String,
    pub cycle_panel: String,
    pub focus_prev: String,
    pub focus_next: String,
    pub mark_answered: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            toggle_capture: "Ctrl+Alt+Space".into(),
            cycle_panel: "Ctrl+Alt+H".into(),
            focus_prev: "Ctrl+Alt+[".into(),
            focus_next: "Ctrl+Alt+]".into(),
            mark_answered: "Ctrl+Alt+Enter".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PanelMode {
    Full,
    Mini,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct OverlaySettings {
    pub click_through: bool,
    pub hide_in_screen_share: bool,
    pub mode: PanelMode,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            click_through: false,
            hide_in_screen_share: true,
            mode: PanelMode::Full,
            x: None,
            y: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PlaybackMode {
    /// Speak questions through the speakers and recognise them like a real meeting.
    RealPipeline,
    /// Speak questions but inject their text after a simulated recognition delay.
    SimulatedLatency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HintLevel {
    Full,
    HeadsOnly,
    Delayed,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PracticeSettings {
    pub playback: PlaybackMode,
    pub hint_level: HintLevel,
    pub hint_delay_s: f32,
    pub answer_pause_s: f32,
    pub nudge_after_s: f32,
    pub follow_up_probability: f32,
    pub speech_rate: f32,
    /// System voice ids for reading questions, per language; empty means the
    /// first voice Windows offers for that language.
    pub voice_zh: String,
    pub voice_en: String,
    pub recognize_me: bool,
    /// Filler words per language, editable in the settings.
    pub fillers_zh: Vec<String>,
    pub fillers_en: Vec<String>,
}

impl Default for PracticeSettings {
    fn default() -> Self {
        Self {
            playback: PlaybackMode::RealPipeline,
            hint_level: HintLevel::Full,
            hint_delay_s: 5.0,
            answer_pause_s: 3.0,
            nudge_after_s: 20.0,
            follow_up_probability: 0.3,
            speech_rate: 1.0,
            voice_zh: String::new(),
            voice_en: String::new(),
            recognize_me: true,
            fillers_zh: ["嗯", "啊", "那个", "就是", "然后"]
                .map(String::from)
                .to_vec(),
            fillers_en: ["um", "uh", "like", "you know"].map(String::from).to_vec(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DiagnosticsSettings {
    /// Write full transcripts to the log. Off by default.
    pub debug_logging: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_files() {
        let s = Settings::default();
        let back = Settings::from_json(&s.to_json()).unwrap();
        assert_eq!(back, s);

        let partial =
            r#"{"general":{"uiLanguage":"zh-CN"},"providers":{"asr":{"provider":"soniox"}}}"#;
        let s = Settings::from_json(partial).unwrap();
        assert_eq!(s.general.ui_language, UiLanguage::ZhCn);
        assert_eq!(s.general.accent, "#F5B841");
        assert_eq!(s.providers.asr.provider.as_deref(), Some("soniox"));
        assert_eq!(s.providers.asr.endpoint, EndpointChoice::Auto);
        assert_eq!(s.hotkeys.toggle_capture, "Ctrl+Alt+Space");
    }

    /// Writes the defaults for the UI's browser preview (next to the bindings).
    #[test]
    fn export_defaults_for_preview() {
        if let Ok(dir) = std::env::var("TS_RS_EXPORT_DIR") {
            let path = std::path::Path::new(&dir).join("../mock/default-settings.json");
            if path.parent().is_some_and(|p| p.is_dir()) {
                std::fs::write(path, Settings::default().to_json() + "\n").unwrap();
            }
        }
    }

    #[test]
    fn old_base_url_moves_to_its_provider() {
        let old = r#"{"providers":{"openaiBaseUrl":"https://host/v1"}}"#;
        let s = Settings::from_json(old).unwrap();
        assert_eq!(
            s.providers
                .base_urls
                .get("openai-compatible")
                .map(String::as_str),
            Some("https://host/v1")
        );
        assert_eq!(s.providers.openai_base_url, None);
        assert!(!s.to_json().contains("openaiBaseUrl"));
    }

    #[test]
    fn endpoint_choice_json() {
        let v = serde_json::to_value(EndpointChoice::Host("api.example.com".into())).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"mode":"host","host":"api.example.com"})
        );
        let v = serde_json::to_value(EndpointChoice::Auto).unwrap();
        assert_eq!(v, serde_json::json!({"mode":"auto"}));
    }

    #[test]
    fn language_resolution() {
        assert_eq!(UiLanguage::System.resolve("zh-Hans-CN"), "zh-CN");
        assert_eq!(UiLanguage::System.resolve("en-US"), "en");
        assert_eq!(UiLanguage::System.resolve("de-DE"), "en");
        assert_eq!(UiLanguage::En.resolve("zh-CN"), "en");
    }
}
