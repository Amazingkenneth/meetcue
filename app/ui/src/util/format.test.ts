import { describe, expect, it } from "vitest";
import { boldSegments, clock, usd } from "./format";

describe("format", () => {
  it("formats clocks", () => {
    expect(clock(156_000)).toBe("02:36");
    expect(clock(3_725_000)).toBe("01:02:05");
    expect(clock(-5)).toBe("00:00");
  });

  it("splits bold markup", () => {
    expect(boldSegments("我们用**双活**架构，**冗余**。")).toEqual([
      { text: "我们用", bold: false },
      { text: "双活", bold: true },
      { text: "架构，", bold: false },
      { text: "冗余", bold: true },
      { text: "。", bold: false },
    ]);
    expect(boldSegments("plain")).toEqual([{ text: "plain", bold: false }]);
    expect(boldSegments("half **open")).toEqual([{ text: "half **open", bold: false }]);
  });

  it("formats dollars", () => {
    expect(usd(1_500_000)).toContain("1.50");
    expect(usd(12_300)).toContain("0.0123");
    expect(usd(null)).toBe("—");
  });
});
