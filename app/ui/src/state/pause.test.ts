import { describe, expect, it } from "vitest";
import { activeMs, NO_PAUSES, trackPause } from "./hooks";

describe("meeting clock", () => {
  it("stops while paused and continues after resuming", () => {
    const start = 1_000_000;
    let c = NO_PAUSES;
    expect(activeMs(start + 10_000, start, c)).toBe(10_000);
    c = trackPause(c, "paused", start + 10_000);
    // Time passes while paused: the clock stays at 10 s.
    expect(activeMs(start + 70_000, start, c)).toBe(10_000);
    c = trackPause(c, "running", start + 70_000);
    expect(activeMs(start + 75_000, start, c)).toBe(15_000);
    // A second pause adds up.
    c = trackPause(c, "paused", start + 80_000);
    c = trackPause(c, "running", start + 90_000);
    expect(activeMs(start + 100_000, start, c)).toBe(30_000);
    expect(trackPause(c, "stopped", start + 100_000)).toEqual(NO_PAUSES);
  });
});
