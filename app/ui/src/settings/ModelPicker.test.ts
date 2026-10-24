import { describe, expect, it } from "vitest";
import type { PriceEntry } from "../bindings/PriceEntry";
import type { PriceTable } from "../bindings/PriceTable";
import type { Settings } from "../bindings/Settings";
import defaults from "../mock/default-settings.json";
import { best, featured, findPrice, namesProvider, snapshotAlias, type Row } from "./ModelPicker";

const entry = { provider: "p", model: "m", label: "", unit: "per_million_tokens", source: "", verified: "" } as PriceEntry;
const row = (model: string, hourly: number | null, latencyMs: number | null = null): Row => ({
  provider: "p",
  model,
  label: model,
  entry: hourly === null ? null : entry,
  hourly,
  latencyMs,
});

describe("model list", () => {
  it("shows the cheapest priced models and keeps the current choice visible", () => {
    const rows = [row("a", 1), row("b", 2), row("c", 3), row("d", 4), row("e", 5), row("x", null)];
    expect(featured(rows, () => false).map((r) => r.model)).toEqual(["a", "b", "c", "d"]);
    expect(featured(rows, (r) => r.model === "x").map((r) => r.model)).toEqual(["a", "b", "c", "d", "x"]);
    // Without any prices, the first few models are shown.
    const unpriced = [row("u", null), row("v", null)];
    expect(featured(unpriced, () => false)).toHaveLength(2);
  });

  it("tags the best row only when there is something to compare", () => {
    expect(best([row("a", 2), row("b", 1)], "hourly")).toBe(1);
    expect(best([row("a", 2), row("b", null)], "hourly")).toBe(-1);
    expect(best([row("a", 1, 300), row("b", 2, 200)], "latencyMs")).toBe(1);
  });

  it("omits the provider when the model name already says it", () => {
    expect(namesProvider("Claude Haiku 5.5", "Anthropic (Claude)")).toBe(true);
    expect(namesProvider("Deepgram Nova-3", "Deepgram")).toBe(true);
    expect(namesProvider("claude-opus-4-1", "Anthropic (Claude)")).toBe(true);
    expect(namesProvider("qwen-max", "OpenAI-compatible")).toBe(false);
  });

  it("prices a dated snapshot like its alias, and lets the user's own price win", () => {
    const settings = structuredClone(defaults) as unknown as Settings;
    const haiku = { ...entry, provider: "anthropic", model: "claude-haiku-4-5", input: 1, output: 5 };
    const table: PriceTable = { updated: "2026-10-24", currency: "USD", entries: [haiku] };
    expect(snapshotAlias("claude-haiku-4-5-20251001")).toBe("claude-haiku-4-5");
    expect(snapshotAlias("nova-3")).toBeNull();
    expect(findPrice(table, settings, "anthropic", "claude-haiku-4-5-20251001")).toBe(haiku);
    expect(findPrice(table, settings, "anthropic", "claude-opus-4-8")).toBeNull();
    const mine = { ...haiku, input: 0.5, source: "user" };
    settings.providers.pricingOverrides = [mine];
    expect(findPrice(table, settings, "anthropic", "claude-haiku-4-5")).toBe(mine);
  });
});
