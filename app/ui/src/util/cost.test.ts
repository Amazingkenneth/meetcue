import { describe, expect, it } from "vitest";
import type { CostAssumptions } from "../bindings/CostAssumptions";
import type { PriceEntry } from "../bindings/PriceEntry";
import { estimateHourly, estimateTask } from "./cost";

// The defaults of CostAssumptions in crates/core/src/pricing.rs.
const a: CostAssumptions = {
  speechRatio: 0.6,
  questionsPerHour: 20,
  systemTokens: 1500,
  detectInputTokens: 900,
  detectOutputTokens: 80,
  hintInputTokens: 1200,
  hintOutputTokens: 500,
  cancelOverhead: 0.1,
};

const entry = (unit: PriceEntry["unit"]): PriceEntry => ({
  provider: "p",
  model: "m",
  label: "",
  unit,
  input: 1,
  output: 5,
  cacheRead: 0.1,
  cacheWrite: 1.25,
  audio: 0.12,
  source: "",
  verified: "",
});

describe("estimated cost per hour", () => {
  it("matches the shell's estimate_hourly (same figures as its test)", () => {
    const est = estimateHourly(entry("per_audio_hour"), entry("per_million_tokens"), null, a);
    expect(est.asr).toBe(144_000);
    expect(est.detect).toBe(31_900);
    expect(est.hints).toBeNull();
    expect(est.complete).toBe(false);
    expect(est.total).toBe(144_000 + 31_900);
  });

  it("converts every price unit", () => {
    expect(estimateTask(entry("per_audio_minute"), "asr", a)).toBe(8_640_000);
    expect(estimateTask(entry("per_thousand_tokens"), "detect", a)).toBe(31_900_000);
    // Hints: (1200 × $1 + 500 × $5 + 1500 × $0.10) per million tokens, 20 questions, +10%.
    expect(estimateTask(entry("per_million_tokens"), "hints", a)).toBe(84_700);
  });

  it("bills the system prompt at the input price when there is no cache price", () => {
    const p = { ...entry("per_million_tokens"), cacheRead: undefined };
    expect(estimateTask(p, "detect", a)).toBe(Math.round(2_800 * 20 * 1.1));
  });
});
