import type { ModelSlot } from "../backend/types";
import type { CostAssumptions } from "../bindings/CostAssumptions";
import type { ModelInfo } from "../bindings/ModelInfo";
import type { PriceEntry } from "../bindings/PriceEntry";
import { estimateTask } from "./cost";

/** One row of the model list: a model with its price, if any is known. */
export interface ModelOption {
  id: string;
  name: string;
  price: PriceEntry | null;
  /** The provider reported this model for the user's account. */
  onAccount: boolean;
  /** Estimated cost of a meeting hour for this task, in micro-dollars. */
  perHour: number | null;
}

/** `model-20250929` → `model`, like `snapshot_alias` in crates/core/src/pricing.rs. */
export function snapshotAlias(model: string): string | null {
  const i = model.lastIndexOf("-");
  if (i < 0) return null;
  const date = model.slice(i + 1);
  return /^\d{8}$/.test(date) ? model.slice(0, i) : null;
}

/** The price for a model: the user's own first, then the price list, then a snapshot's alias. */
export function findPrice(
  entries: PriceEntry[],
  overrides: PriceEntry[],
  provider: string,
  model: string,
): PriceEntry | null {
  const all = [...overrides, ...entries];
  const exact = all.find((e) => e.provider === provider && e.model === model);
  if (exact) return exact;
  const alias = snapshotAlias(model);
  return (alias && all.find((e) => e.provider === provider && e.model === alias)) || null;
}

/** Whether a price entry fits the task: audio prices for recognition, token prices for models. */
function fits(e: PriceEntry, slot: ModelSlot): boolean {
  const audio = e.unit.startsWith("per_audio");
  return slot === "asr" ? audio : !audio;
}

/**
 * Listed models for the provider, then any others its account reports. A
 * model the account reports keeps the listed price; one it doesn't report is
 * still offered, since the list loads only once a key is saved.
 */
export function modelOptions(
  provider: string,
  slot: ModelSlot,
  entries: PriceEntry[],
  overrides: PriceEntry[],
  reported: ModelInfo[],
  assumptions: CostAssumptions,
): ModelOption[] {
  const out: ModelOption[] = [];
  const seen = new Set<string>();
  const reportedIds = new Set(reported.map((m) => m.id));
  const add = (id: string, name: string, price: PriceEntry | null, onAccount: boolean) => {
    if (seen.has(id)) return;
    seen.add(id);
    const p = price && fits(price, slot) ? price : null;
    out.push({ id, name, price: p, onAccount, perHour: p ? estimateTask(p, slot, assumptions) : null });
  };
  for (const e of [...overrides, ...entries]) {
    if (e.provider !== provider || !fits(e, slot)) continue;
    add(e.model, e.label || e.model, findPrice(entries, overrides, provider, e.model), reportedIds.has(e.model));
  }
  for (const m of reported) {
    const name = m.displayName && m.displayName !== m.id ? m.displayName : m.id;
    add(m.id, name, findPrice(entries, overrides, provider, m.id) ?? m.price ?? null, true);
  }
  return out;
}

/** Options whose id or name contains every typed word. */
export function filterOptions(options: ModelOption[], query: string): ModelOption[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return options;
  return options.filter((o) => {
    const hay = `${o.id} ${o.name}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** Audio price per hour, whatever unit the provider quotes. */
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

/** Token prices per million tokens, whatever unit the provider quotes. */
export function tokensPerMillion(p: PriceEntry): { input: number; output: number } | null {
  const k = p.unit === "per_million_tokens" ? 1 : p.unit === "per_thousand_tokens" ? 1000 : 0;
  if (!k || (p.input === undefined && p.output === undefined)) return null;
  return { input: (p.input ?? 0) * k, output: (p.output ?? 0) * k };
}
