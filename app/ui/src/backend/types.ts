// The command and event contract between the UI and the desktop shell.
// Payload types are generated from Rust (src/bindings); this file only maps
// command names to their argument and result types.

import type { AppEvent } from "../bindings/AppEvent";
import type { AppInfo } from "../bindings/AppInfo";
import type { DeviceList } from "../bindings/DeviceList";
import type { HotkeyState } from "../bindings/HotkeyState";
import type { HourlyEstimate } from "../bindings/HourlyEstimate";
import type { LogLine } from "../bindings/LogLine";
import type { CostAssumptions } from "../bindings/CostAssumptions";
import type { Material } from "../bindings/Material";
import type { MeetCueEvent } from "../bindings/MeetCueEvent";
import type { ModelInfo } from "../bindings/ModelInfo";
import type { PanelMode } from "../bindings/PanelMode";
import type { PriceEntry } from "../bindings/PriceEntry";
import type { PriceTable } from "../bindings/PriceTable";
import type { ProbeReport } from "../bindings/ProbeReport";
import type { ProbeRequest } from "../bindings/ProbeRequest";
import type { PracticeScript } from "../bindings/PracticeScript";
import type { PracticeReport } from "../bindings/PracticeReport";
import type { PracticeReportSummary } from "../bindings/PracticeReportSummary";
import type { Settings } from "../bindings/Settings";
import type { TimedEvent } from "../bindings/TimedEvent";
import type { UsageSummary } from "../bindings/UsageSummary";
import type { UserAction } from "../bindings/UserAction";
import type { VerifyResult } from "../bindings/VerifyResult";

export type ModelSlot = "asr" | "detect" | "hints";
export type View = "overlay" | "settings" | "wizard" | "practice" | "review";
export type PracticeControl = "pause" | "resume" | "skip" | "stop" | "answered";

export interface Commands {
  get_app_info: { args: undefined; result: AppInfo };
  get_settings: { args: undefined; result: Settings };
  save_settings: { args: { settings: Settings }; result: Settings };
  list_devices: { args: undefined; result: DeviceList };
  credential_status: { args: undefined; result: Record<string, boolean> };
  set_credential: { args: { key: string; value: string }; result: null };
  delete_credential: { args: { key: string }; result: null };
  verify_provider: { args: { provider: string }; result: VerifyResult };
  list_models: { args: { provider: string }; result: ModelInfo[] };
  get_pricing: { args: undefined; result: PriceTable };
  refresh_pricing: { args: { force: boolean }; result: PriceTable };
  estimate_cost: {
    args: { asr: PriceEntry | null; detect: PriceEntry | null; hints: PriceEntry | null; assumptions: CostAssumptions };
    result: HourlyEstimate;
  };
  provider_hosts: { args: { provider: string }; result: string[] };
  audio_preview: { args: { on: boolean }; result: null };
  start_session: { args: undefined; result: null };
  stop_session: { args: undefined; result: null };
  toggle_capture: { args: undefined; result: null };
  user_action: { args: { action: UserAction }; result: null };
  session_history: { args: undefined; result: TimedEvent[] };
  set_panel_mode: { args: { mode: PanelMode }; result: null };
  set_always_on_top: { args: { on: boolean }; result: null };
  open_window: { args: { view: View; section?: string }; result: null };
  open_url: { args: { url: string }; result: null };
  hotkey_status: { args: undefined; result: HotkeyState[] };
  recent_logs: { args: { after: number }; result: LogLine[] };
  run_probe: { args: { request: ProbeRequest }; result: ProbeReport };
  probe_history: { args: undefined; result: ProbeReport[] };
  usage_summary: { args: { month: string | null }; result: UsageSummary };
  export_usage_csv: { args: undefined; result: string | null };
  diagnostics_preview: { args: undefined; result: string[] };
  export_diagnostics: { args: undefined; result: string | null };
  import_material: { args: undefined; result: Material | null };
  practice_import: { args: undefined; result: PracticeScript | null };
  practice_classify: { args: { script: PracticeScript }; result: PracticeScript };
  practice_start: { args: { script: PracticeScript }; result: null };
  practice_control: { args: { control: PracticeControl }; result: null };
  practice_reports: { args: undefined; result: PracticeReportSummary[] };
  practice_report: { args: { id: string }; result: PracticeReport };
  export_report: { args: { id: string }; result: string | null };
}

export type CommandName = keyof Commands;
export type Unlisten = () => void;

export interface Backend {
  readonly kind: "tauri" | "mock";
  invoke<K extends CommandName>(
    cmd: K,
    ...args: Commands[K]["args"] extends undefined ? [] : [Commands[K]["args"]]
  ): Promise<Commands[K]["result"]>;
  onEvent(cb: (e: MeetCueEvent) => void): Promise<Unlisten>;
  onAppEvent(cb: (e: AppEvent) => void): Promise<Unlisten>;
  /** Closes the window this view runs in. */
  closeWindow(): Promise<void>;
}
