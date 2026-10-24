import type { ModelSlot } from "../backend/types";
import type { CostAssumptions } from "../bindings/CostAssumptions";
import type { HourlyEstimate } from "../bindings/HourlyEstimate";
import type { PriceEntry } from "../bindings/PriceEntry";

// Estimated cost of an hour of meeting. Mirrors `estimate_hourly` in
// crates/core/src/pricing.rs step by step, including rounding to whole
// micro-dollars, so the settings show the same figures the shell computes;
// both are checked against the same numbers in their tests.

const micros = (usd: number) => Math.max(0, Math.round(usd * 1_000_000));

function perToken(p: PriceEntry): number {
  return p.unit === "per_million_tokens" ? 1e-6 : p.unit === "per_thousand_tokens" ? 1e-3 : 0;
}

function perAudioMs(p: PriceEntry): number {
  switch (p.unit) {
    case "per_audio_hour":
      return 1 / 3_600_000;
    case "per_audio_minute":
      return 1 / 60_000;
    case "per_audio_second":
      return 1 / 1000;
    default:
      return 0;
  }
}

/** Speech recognition: both channels for the share of the hour with speech. */
export function asrPerHour(p: PriceEntry, a: CostAssumptions): number {
  const ms = Math.trunc(3_600_000 * 2 * a.speechRatio);
  return micros(ms * (p.audio ?? 0) * perAudioMs(p));
}

/**
 * A model task: one call per question (plus cancelled early triggers), with
 * the stable system prompt billed at the cache-read price.
 */
export function llmPerHour(p: PriceEntry, input: number, output: number, a: CostAssumptions): number {
  const k = perToken(p);
  const inputPrice = p.input ?? 0;
  const call = micros(
    Math.trunc(input) * inputPrice * k + Math.trunc(output) * (p.output ?? 0) * k + Math.trunc(a.systemTokens) * (p.cacheRead ?? inputPrice) * k,
  );
  return Math.round(call * a.questionsPerHour * (1 + a.cancelOverhead));
}

/** Estimated cost per hour of using `p` for one task, in micro-dollars. */
export function estimateTask(p: PriceEntry, slot: ModelSlot, a: CostAssumptions): number {
  if (slot === "asr") return asrPerHour(p, a);
  return slot === "detect"
    ? llmPerHour(p, a.detectInputTokens, a.detectOutputTokens, a)
    : llmPerHour(p, a.hintInputTokens, a.hintOutputTokens, a);
}

/** Estimated cost per hour of a whole setup; unknown parts are left out of the total. */
export function estimateHourly(
  asr: PriceEntry | null,
  detect: PriceEntry | null,
  hints: PriceEntry | null,
  a: CostAssumptions,
): HourlyEstimate {
  const parts = [asr && estimateTask(asr, "asr", a), detect && estimateTask(detect, "detect", a), hints && estimateTask(hints, "hints", a)];
  return {
    asr: parts[0],
    detect: parts[1],
    hints: parts[2],
    total: parts.reduce<number>((s, x) => s + (x ?? 0), 0),
    complete: parts.every((x) => x !== null),
  };
}
