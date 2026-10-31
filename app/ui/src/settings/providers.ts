import type { ModelSlot } from "../backend/types";
import type { Settings } from "../bindings/Settings";

export interface ProviderMeta {
  id: string;
  kind: "asr" | "llm";
  name: string;
  /** Whether the user can point the provider at an address of their own. */
  baseUrl?: "required" | "optional";
  /** The address used when no base URL is set; shown as the placeholder. */
  defaultBaseUrl?: string;
  keyOptional?: boolean;
  /** Locale key of the key field's placeholder, when the key isn't a single token. */
  keyFormat?: string;
  /** One model name shown as an example in the empty model field. */
  modelExample?: string;
  consoleUrl?: string;
  /** The provider's own price page. MeetCue does not list prices itself. */
  pricingUrl?: string;
  available: boolean;
}

export const PROVIDERS: ProviderMeta[] = [
  {
    id: "soniox",
    kind: "asr",
    name: "Soniox",
    modelExample: "stt-rt-v5",
    consoleUrl: "https://console.soniox.com/",
    pricingUrl: "https://soniox.com/pricing",
    available: true,
  },
  {
    id: "deepgram",
    kind: "asr",
    name: "Deepgram",
    modelExample: "nova-3",
    consoleUrl: "https://console.deepgram.com/",
    pricingUrl: "https://deepgram.com/pricing",
    available: true,
  },
  {
    id: "alibaba",
    kind: "asr",
    name: "Alibaba Cloud (Fun-ASR)",
    modelExample: "fun-asr-realtime",
    consoleUrl: "https://modelstudio.console.alibabacloud.com/?tab=playground#/api-key",
    pricingUrl: "https://www.alibabacloud.com/help/en/model-studio/model-pricing",
    available: true,
  },
  {
    id: "tencent",
    kind: "asr",
    name: "Tencent Cloud ASR",
    modelExample: "16k_zh_en",
    keyFormat: "providers.keyTencent",
    consoleUrl: "https://console.cloud.tencent.com/cam/capi",
    pricingUrl: "https://cloud.tencent.com/document/product/1093/35686",
    available: true,
  },
  { id: "azure-speech", kind: "asr", name: "Azure Speech", available: false },
  {
    id: "anthropic",
    kind: "llm",
    name: "Anthropic (Claude)",
    baseUrl: "optional",
    defaultBaseUrl: "https://api.anthropic.com",
    modelExample: "claude-haiku-5-5",
    consoleUrl: "https://platform.claude.com/",
    pricingUrl: "https://platform.claude.com/docs/en/about-claude/pricing",
    available: true,
  },
  {
    id: "openai-compatible",
    kind: "llm",
    name: "OpenAI-compatible",
    baseUrl: "required",
    keyOptional: true,
    available: true,
  },
];

export function providersFor(slot: ModelSlot): ProviderMeta[] {
  return PROVIDERS.filter((p) => p.kind === (slot === "asr" ? "asr" : "llm"));
}

export function providerMeta(id: string | null | undefined): ProviderMeta | undefined {
  return PROVIDERS.find((p) => p.id === id);
}

export function providerName(id: string | null | undefined): string {
  return providerMeta(id)?.name ?? id ?? "—";
}

/** The user's base URL for a provider, if they set one. */
export function baseUrlOf(settings: Settings, id: string): string | null {
  return settings.providers.baseUrls[id]?.trim() || null;
}

/** Whether a provider has everything but its key: a base URL where one is required. */
export function reachable(settings: Settings, p: ProviderMeta): boolean {
  return p.baseUrl !== "required" || !!baseUrlOf(settings, p.id);
}

/** Providers chosen for at least one task, in the order the tasks are listed. */
export function providersInUse(settings: Settings): ProviderMeta[] {
  const ids = (["asr", "detect", "hints"] as const).map((s) => settings.providers[s].provider);
  return PROVIDERS.filter((p) => ids.includes(p.id));
}
