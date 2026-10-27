//! Data exchanged between the desktop shell and the UI (besides pipeline
//! events). Kept here so the TypeScript bindings are generated with the rest.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::pricing::PriceTable;
use crate::settings::{PanelHeight, PanelMode, PanelPlace, PanelWidth, Settings};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    /// `windows`, `macos`, `linux` or `web` (browser preview).
    pub platform: String,
    /// Resolved UI language: `en` or `zh-CN`.
    pub language: String,
    pub system_locale: String,
    pub portable: bool,
    pub config_dir: String,
    /// Running with mock providers (`--demo` or the browser preview).
    pub demo: bool,
    pub docs_url: String,
    /// Whether audio capture works on this platform yet.
    pub capture_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// Bluetooth headsets switch to a low-quality call profile when their
    /// microphone is opened; the settings page warns about it.
    pub is_bluetooth: bool,
}

/// A system text-to-speech voice offered for reading practice questions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// Primary language, such as `en` or `zh`.
    pub lang: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceList {
    pub microphones: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VerifyResult {
    pub ok: bool,
    pub code: Option<String>,
    pub detail: Option<String>,
}

impl VerifyResult {
    pub fn ok() -> Self {
        Self {
            ok: true,
            code: None,
            detail: None,
        }
    }

    pub fn err(e: &crate::MeetCueError) -> Self {
        Self {
            ok: false,
            code: Some(e.code.clone()),
            detail: Some(e.detail.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HotkeyState {
    /// Settings field name, e.g. `toggleCapture`.
    pub action: String,
    pub accelerator: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageRow {
    pub provider: String,
    pub model: String,
    pub calls: u32,
    #[ts(type = "number")]
    pub input_tokens: u64,
    #[ts(type = "number")]
    pub output_tokens: u64,
    #[ts(type = "number")]
    pub audio_ms: u64,
    /// `None` when the price of this model is unknown.
    #[ts(type = "number | null")]
    pub micros: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageSummary {
    /// `YYYY-MM`.
    pub month: String,
    pub rows: Vec<UsageRow>,
    #[ts(type = "number")]
    pub total_micros: u64,
}

/// One line of the application log, for the activity log view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LogLine {
    /// Increases with every line; pass the last one seen to get newer lines.
    #[ts(type = "number")]
    pub seq: u64,
    pub line: String,
}

/// App-level notifications on the `meetcue://app` channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum AppEvent {
    SettingsChanged {
        settings: Box<Settings>,
    },
    PanelMode {
        mode: PanelMode,
    },
    HotkeyStatus {
        hotkeys: Vec<HotkeyState>,
    },
    /// A one-off notice for an error code (e.g. device changed).
    Notice {
        code: String,
        detail: String,
    },
    ProbeProgress {
        done: u32,
        total: u32,
        label: String,
    },
    PracticeProgress {
        index: u32,
        total: u32,
        /// `speaking`, `waiting`, `answering`, `followUp`, `paused`, `done`.
        phase: String,
        question: String,
    },
    PracticeFinished {
        report_id: String,
    },
    /// A newer price list was loaded.
    PricingChanged {
        pricing: Box<PriceTable>,
    },
    /// The panel went to a saved spot, or the current one was saved.
    PanelSpot {
        slot: u8,
        saved: bool,
    },
}

/// A change to where the panel is, how big it is or how it lets the meeting
/// through, from the panel's placement menu or the settings window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum PanelChange {
    Snap {
        place: PanelPlace,
    },
    /// Same place on the next screen.
    NextScreen,
    Width {
        width: PanelWidth,
    },
    Height {
        height: PanelHeight,
    },
    /// Go to a saved spot (`slot` counts from 0); an empty one saves the current spot.
    GoSpot {
        slot: u8,
    },
    SaveSpot {
        slot: u8,
    },
    ClearSpot {
        slot: u8,
    },
    /// Hide for a few seconds; again to come back at once.
    Peek,
    ClickThrough {
        on: bool,
    },
    Opacity {
        value: f32,
    },
    TextScale {
        value: f32,
    },
}
