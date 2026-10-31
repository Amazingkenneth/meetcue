import { describe, expect, it } from "vitest";
import type { CostAssumptions } from "../bindings/CostAssumptions";
import type { PriceEntry } from "../bindings/PriceEntry";
import { audioPerHour, filterOptions, findPrice, modelOptions, snapshotAlias, tokensPerMillion } from "./models";

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

const price = (provider: string, model: string, extra: Partial<PriceEntry>): PriceEntry => ({
  provider,
  model,
  label: model,
  unit: "per_audio_hour",
  source: "",
  verified: "",
  ...extra,
});

const table = [
  price("soniox", "stt-rt-v5", { label: "Soniox stt-rt-v5", audio: 0.12 }),
  price("deepgram", "nova-3", { unit: "per_audio_minute", audio: 0.0077 }),
  price("anthropic", "claude-haiku-5-5", { label: "Claude Haiku 5.5", unit: "per_million_tokens", input: 0.1, output: 0.5 }),
];

describe("model options", () => {
  it("lists priced models before a key is saved, with the meeting-hour estimate", () => {
    const o = modelOptions("soniox", "asr", table, [], [], a);
    expect(o).toHaveLength(1);
    expect(o[0]).toMatchObject({ id: "stt-rt-v5", name: "Soniox stt-rt-v5", onAccount: false });
    // 0.12/h × 2 channels × 60% speech.
    expect(o[0].perHour).toBe(144_000);
  });

  it("adds reported models and marks listed ones the account has", () => {
    const o = modelOptions(
      "soniox",
      "asr",
      table,
      [],
      [
        { id: "stt-rt-v5", displayName: "stt-rt-v5" },
        { id: "stt-rt-v6", displayName: "Soniox v6" },
      ],
      a,
    );
    expect(o.map((x) => [x.id, x.onAccount, x.perHour !== null])).toEqual([
      ["stt-rt-v5", true, true],
      ["stt-rt-v6", true, false],
    ]);
    expect(o[1].name).toBe("Soniox v6");
  });

  it("prefers the user's own price and keeps token prices out of recognition", () => {
    const own = [price("soniox", "stt-rt-v5", { audio: 0.2, source: "user" })];
    expect(modelOptions("soniox", "asr", table, own, [], a)[0].price?.audio).toBe(0.2);
    expect(modelOptions("anthropic", "asr", table, [], [], a)).toEqual([]);
    expect(modelOptions("anthropic", "hints", table, [], [], a)[0].id).toBe("claude-haiku-5-5");
  });

  it("finds a snapshot's price under its alias", () => {
    expect(snapshotAlias("claude-haiku-5-5-20261001")).toBe("claude-haiku-5-5");
    expect(snapshotAlias("nova-3")).toBeNull();
    expect(findPrice(table, [], "anthropic", "claude-haiku-5-5-20261001")?.model).toBe("claude-haiku-5-5");
    expect(findPrice(table, [], "anthropic", "other")).toBeNull();
  });

  it("filters by every typed word", () => {
    const o = modelOptions("anthropic", "hints", table, [], [{ id: "claude-opus-5-5", displayName: "Claude Opus 5.5" }], a);
    expect(filterOptions(o, "").length).toBe(2);
    expect(filterOptions(o, "opus").map((x) => x.id)).toEqual(["claude-opus-5-5"]);
    expect(filterOptions(o, "claude 5.5").length).toBe(2);
    expect(filterOptions(o, "gpt")).toEqual([]);
  });

  it("converts prices to per hour and per million", () => {
    expect(audioPerHour(table[1])).toBeCloseTo(0.462);
    expect(audioPerHour(table[0])).toBe(0.12);
    expect(tokensPerMillion(table[2])).toEqual({ input: 0.1, output: 0.5 });
    expect(tokensPerMillion(price("x", "y", { unit: "per_thousand_tokens", input: 0.001, output: 0.002 }))).toEqual({ input: 1, output: 2 });
    expect(audioPerHour(table[2])).toBeNull();
  });
});
