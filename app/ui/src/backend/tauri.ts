import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AppEvent } from "../bindings/AppEvent";
import type { MeetCueEvent } from "../bindings/MeetCueEvent";
import type { Backend, CommandName, Commands } from "./types";

export const PIPELINE_EVENT = "meetcue://event";
export const APP_EVENT = "meetcue://app";

export const tauriBackend: Backend = {
  kind: "tauri",
  invoke<K extends CommandName>(cmd: K, ...args: unknown[]) {
    return invoke<Commands[K]["result"]>(cmd, (args[0] as Record<string, unknown>) ?? {});
  },
  async onEvent(cb) {
    return listen<MeetCueEvent>(PIPELINE_EVENT, (e) => cb(e.payload));
  },
  async onAppEvent(cb) {
    return listen<AppEvent>(APP_EVENT, (e) => cb(e.payload));
  },
  // The DOM's window.close() tears down the page in WebView2 but leaves the
  // native window open and blank, so close it through the shell.
  closeWindow() {
    return getCurrentWindow().close();
  },
};
