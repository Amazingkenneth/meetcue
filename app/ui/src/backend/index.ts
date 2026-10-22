import type { Backend } from "./types";
import { tauriBackend } from "./tauri";
import { createMockBackend } from "../mock/mockBackend";

let instance: Backend | null = null;

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** The desktop shell when running inside it, otherwise the scripted mock. */
export function backend(): Backend {
  if (!instance) {
    instance = !__MOCK__ && inTauri() ? tauriBackend : createMockBackend();
  }
  return instance;
}

/** Replaces the backend (tests). */
export function setBackend(b: Backend): void {
  instance = b;
}

export type { Backend } from "./types";
