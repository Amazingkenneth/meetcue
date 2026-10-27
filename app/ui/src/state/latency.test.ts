import { describe, expect, it } from "vitest";
import defaults from "../mock/default-settings.json";
import type { Settings } from "../bindings/Settings";
import { latencyTip, type LatencyBreakdown } from "./latency";

const settings = (f: (s: Settings) => void = () => undefined): Settings => {
  const s = structuredClone(defaults) as unknown as Settings;
  f(s);
  return s;
};
const sample = (asrMs: number, detectMs: number, generateMs: number): LatencyBreakdown => ({
  asrMs,
  detectMs,
  generateMs,
  totalMs: asrMs + generateMs,
  early: false,
});

describe("latencyTip", () => {
  it("suggests writing hints during detection when that is off", () => {
    const s = settings((s) => (s.hints.speculativeHints = false));
    expect(latencyTip(sample(400, 900, 1200), s)).toEqual({ key: "speculative", section: "hints" });
  });

  it("points at the sentence end wait when recognition is the slow part", () => {
    expect(latencyTip(sample(1500, 300, 800), settings())).toEqual({ key: "endpoint", section: "audio" });
    const noEarly = settings((s) => (s.hints.earlyTrigger = false));
    expect(latencyTip(sample(1500, 300, 800), noEarly).key).toBe("earlyTrigger");
  });

  it("names the model that holds up the first point", () => {
    expect(latencyTip(sample(400, 1500, 900), settings()).key).toBe("detectModel");
    expect(latencyTip(sample(400, 500, 2500), settings()).key).toBe("hintModel");
  });

  it("falls back to the latency probe", () => {
    expect(latencyTip(sample(400, 300, 900), settings())).toEqual({ key: "network", section: "probe" });
  });
});
