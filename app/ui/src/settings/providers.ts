import type { PriceEntry } from "../bindings/PriceEntry";
import type { ModelSlot } from "../backend/types";

export interface ProviderMeta {
  id: string;
  kind: "asr" | "llm";
  name: string;
  /** Extra settings the provider needs besides its key. */
  needs?: "baseUrl";
  keyOptional?: boolean;
  consoleUrl?: string;
  /** The provider's own price page, shown when MeetCue has no figure for a model. */
  pricingUrl?: string;
  available: boolean;
}

export const PROVIDERS: ProviderMeta[] = [
  {
    id: "soniox",
    kind: "asr",
    name: "Soniox",
    consoleUrl: "https://console.soniox.com/",
    pricingUrl: "https://soniox.com/pricing",
    available: true,
  },
  {
    id: "deepgram",
    kind: "asr",
    name: "Deepgram",
    consoleUrl: "https://console.deepgram.com/",
    pricingUrl: "https://deepgram.com/pricing",
    available: true,
  },
  { id: "azure-speech", kind: "asr", name: "Azure Speech", available: false },
  {
    id: "anthropic",
    kind: "llm",
    name: "Anthropic (Claude)",
    consoleUrl: "https://platform.claude.com/",
    pricingUrl: "https://platform.claude.com/docs/en/about-claude/pricing",
    available: true,
  },
  { id: "openai-compatible", kind: "llm", name: "OpenAI-compatible", needs: "baseUrl", keyOptional: true, available: true },
];

export function providersFor(slot: ModelSlot): ProviderMeta[] {
  return PROVIDERS.filter((p) => p.kind === (slot === "asr" ? "asr" : "llm"));
}

export function providerName(id: string | null | undefined): string {
  return PROVIDERS.find((p) => p.id === id)?.name ?? id ?? "—";
}

/** Price shown in the comparison: per hour for audio, per million tokens otherwise. */
export function audioPerHour(p: PriceEntry): number | null {
  if (p.audio === undefined) return null;
  switch (p.unit) {
    case "per_audio_hour":
      return p.audio;
    case "per_audio_minute":
      return p.audio * 60;
    case "per_audio_second":
      return p.audio * 3600;
    default:
      return null;
  }
}

export function tokensPerMillion(value: number | undefined, unit: PriceEntry["unit"]): number | null {
  if (value === undefined) return null;
  return unit === "per_thousand_tokens" ? value * 1000 : value;
}
