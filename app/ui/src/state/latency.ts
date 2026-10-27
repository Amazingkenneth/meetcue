// Turns the timing of recent hints into one concrete step that makes them
// faster, based on which stage took the most time.

import type { Settings } from "../bindings/Settings";

export interface LatencyBreakdown {
  asrMs: number;
  detectMs: number;
  generateMs: number;
  totalMs: number;
  early: boolean;
}

export interface LatencyTip {
  /** Translation key under `latencyTips`. */
  key: "speculative" | "earlyTrigger" | "endpoint" | "detectModel" | "hintModel" | "network";
  /** Settings section that holds the fix. */
  section: "hints" | "audio" | "providers" | "probe";
}

/** Waiting for the end of a sentence longer than this is worth shortening. */
const SLOW_RECOGNITION_MS = 900;

/** The single most useful change, given the last hint's timing and the settings. */
export function latencyTip(b: LatencyBreakdown, s: Settings): LatencyTip {
  if (!s.hints.speculativeHints && b.detectMs > 300) return { key: "speculative", section: "hints" };
  if (b.asrMs >= b.detectMs && b.asrMs >= b.generateMs && b.asrMs > SLOW_RECOGNITION_MS) {
    if (!s.hints.earlyTrigger) return { key: "earlyTrigger", section: "hints" };
    if (s.audio.endpointSilenceMs > 500) return { key: "endpoint", section: "audio" };
    return { key: "network", section: "probe" };
  }
  // With speculative hints, detection overlaps generation: only a detection
  // slower than the hints themselves still holds up the first point.
  if (b.detectMs > b.generateMs && b.detectMs > 600) return { key: "detectModel", section: "providers" };
  if (b.generateMs > 1200) return { key: "hintModel", section: "providers" };
  return { key: "network", section: "probe" };
}
