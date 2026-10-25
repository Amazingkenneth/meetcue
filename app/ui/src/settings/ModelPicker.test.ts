import { describe, expect, it } from "vitest";
import type { Settings } from "../bindings/Settings";
import defaults from "../mock/default-settings.json";
import { allChosen, chooseProvider } from "./ModelPicker";
import { baseUrlOf, providerMeta, providersInUse, reachable } from "./providers";

const fresh = () => structuredClone(defaults) as unknown as Settings;

describe("provider choice", () => {
  it("needs a provider and a model name for every task", () => {
    const s = fresh();
    expect(allChosen(s)).toBe(false);
    s.providers.asr = { ...s.providers.asr, provider: "soniox", model: "any-model-name" };
    s.providers.detect = { ...s.providers.detect, provider: "openai-compatible", model: "my-local-model" };
    expect(allChosen(s)).toBe(false);
    s.providers.hints = { ...s.providers.hints, provider: "anthropic", model: "claude-next" };
    expect(allChosen(s)).toBe(true);
  });

  it("clears the model when the provider changes, and keeps it otherwise", () => {
    const s = fresh();
    s.providers.hints = { ...s.providers.hints, provider: "anthropic", model: "claude-haiku-5-5" };
    expect(chooseProvider(s, "hints", "anthropic").providers.hints.model).toBe("claude-haiku-5-5");
    const changed = chooseProvider(s, "hints", "openai-compatible").providers.hints;
    expect(changed).toMatchObject({ provider: "openai-compatible", model: null });
  });

  it("lists each provider in use once and needs a base URL only where one is required", () => {
    const s = fresh();
    s.providers.detect = { ...s.providers.detect, provider: "openai-compatible", model: "m" };
    s.providers.hints = { ...s.providers.hints, provider: "openai-compatible", model: "m" };
    expect(providersInUse(s).map((p) => p.id)).toEqual(["openai-compatible"]);
    const compat = providerMeta("openai-compatible")!;
    expect(reachable(s, compat)).toBe(false);
    s.providers.baseUrls["openai-compatible"] = " https://host/v1 ";
    expect(reachable(s, compat)).toBe(true);
    expect(baseUrlOf(s, "openai-compatible")).toBe("https://host/v1");
    expect(reachable(s, providerMeta("anthropic")!)).toBe(true);
  });
});
