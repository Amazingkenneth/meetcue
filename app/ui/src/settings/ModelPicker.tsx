import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { ModelSlot } from "../backend/types";
import type { HourlyEstimate } from "../bindings/HourlyEstimate";
import type { PriceEntry } from "../bindings/PriceEntry";
import type { PriceTable } from "../bindings/PriceTable";
import type { ProbeReport } from "../bindings/ProbeReport";
import type { Settings } from "../bindings/Settings";
import { day, millis, price, usd } from "../util/format";
import { audioPerHour, PROVIDERS, providerName, providersFor, tokensPerMillion, type ProviderMeta } from "./providers";

type Update = (f: (s: Settings) => Settings) => Promise<void>;

export interface Row {
  provider: string;
  model: string;
  label: string;
  entry: PriceEntry | null;
  hourly: number | null;
  latencyMs: number | null;
}

const SLOTS: ModelSlot[] = ["asr", "detect", "hints"];

export function findPrice(table: PriceTable | null, settings: Settings, provider: string, model: string): PriceEntry | null {
  const o = settings.providers.pricingOverrides.find((e) => e.provider === provider && e.model === model);
  if (o) return o;
  return table?.entries.find((e) => e.provider === provider && e.model === model) ?? null;
}

export function latencyFor(probe: ProbeReport | null, slot: ModelSlot, provider: string, model: string): number | null {
  if (!probe) return null;
  const matches = probe.results.filter((r) => r.provider === provider && (r.model === model || r.model === null) && !r.error);
  const values = matches
    .map((r) => (slot === "asr" ? r.finalResult?.medianMs : (r.firstPoint ?? r.firstToken)?.medianMs))
    .filter((v): v is number => typeof v === "number");
  return values.length ? Math.min(...values) : null;
}

/** Index of the row with the lowest value of `key`, if at least two rows have one. */
export function best(rows: Row[], key: "hourly" | "latencyMs"): number {
  const known = rows.map((r, i) => ({ v: r[key], i })).filter((x): x is { v: number; i: number } => x.v !== null);
  if (known.length < 2) return -1;
  return known.reduce((a, b) => (b.v < a.v ? b : a)).i;
}

/** Rows shown before "Show all": the cheapest few priced models plus the current choice. */
export function featured(rows: Row[], isSelected: (r: Row) => boolean, limit = 4): Row[] {
  const priced = rows.filter((r) => r.entry).slice(0, limit);
  const pool = priced.length ? priced : rows.slice(0, limit);
  return rows.filter((r) => pool.includes(r) || isSelected(r));
}

/** Whether a model's name already says who makes it ("Claude Haiku 5.5" for Anthropic (Claude)). */
export function namesProvider(label: string, providerName: string): boolean {
  const l = label.toLowerCase();
  return providerName
    .split(/[\s()]+/)
    .filter((w) => w.length > 2)
    .some((w) => l.includes(w.toLowerCase()));
}

/** Models for every task, with one note on where the prices come from. */
export function ModelsPanel({
  settings,
  update,
  table,
  probe,
  credentials,
  asrExtra,
  footer,
}: {
  settings: Settings;
  update: Update;
  table: PriceTable | null;
  probe: ProbeReport | null;
  credentials: Record<string, boolean>;
  asrExtra?: ReactNode;
  footer?: ReactNode;
}) {
  const { t } = useTranslation();
  const [refreshing, setRefreshing] = useState(false);
  const [failed, setFailed] = useState(false);
  const loaded = !!table?.entries.length;
  const refresh = () => {
    setRefreshing(true);
    setFailed(false);
    void backend()
      .invoke("refresh_pricing")
      .catch(() => setFailed(true))
      .finally(() => setRefreshing(false));
  };
  return (
    <div className="models">
      {SLOTS.map((slot) => (
        <div key={slot} className="model-slot">
          <h3>{t(`providers.${slot}Title`)}</h3>
          <p className="muted">{t(`providers.${slot}Lead`)}</p>
          <ModelPicker slot={slot} settings={settings} update={update} table={table} probe={probe} credentials={credentials} />
          {slot === "asr" && asrExtra}
        </div>
      ))}
      <p className="price-note">
        {loaded ? t("providers.priceSource", { date: day(table!.updated) }) : t("providers.priceMissing")}{" "}
        <button className="btn-link" disabled={refreshing} onClick={refresh}>
          {refreshing ? t("providers.refreshing") : t("providers.refreshPrices")}
        </button>
        {failed && <span className="price-failed"> · {t("providers.refreshFailed")}</span>}
      </p>
      {footer}
    </div>
  );
}

export function ModelPicker({
  slot,
  settings,
  update,
  table,
  probe,
  credentials,
}: {
  slot: ModelSlot;
  settings: Settings;
  update: Update;
  table: PriceTable | null;
  probe: ProbeReport | null;
  credentials: Record<string, boolean>;
}) {
  const { t } = useTranslation();
  const [listed, setListed] = useState<Record<string, { id: string; displayName: string }[]>>({});
  const [estimates, setEstimates] = useState<Record<string, number | null>>({});
  const [showAll, setShowAll] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const providers = useMemo(() => providersFor(slot).filter((p) => p.available), [slot]);
  const choice = settings.providers[slot];

  // Fetch model lists from providers whose key is configured.
  useEffect(() => {
    let alive = true;
    for (const p of providers) {
      if (!credentials[p.id] && !p.keyOptional) continue;
      if (p.needs === "baseUrl" && !settings.providers.openaiBaseUrl) continue;
      void backend()
        .invoke("list_models", { provider: p.id })
        .then((models) => alive && setListed((m) => ({ ...m, [p.id]: models })))
        .catch(() => undefined);
    }
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [slot, JSON.stringify(credentials), settings.providers.openaiBaseUrl]);

  const baseRows = useMemo(() => {
    const out: Omit<Row, "hourly" | "latencyMs">[] = [];
    const seen = new Set<string>();
    for (const p of providers) {
      const fromTable = (table?.entries ?? []).filter((e) => e.provider === p.id);
      const fromApi = listed[p.id] ?? [];
      const current = choice.provider === p.id && choice.model ? [{ id: choice.model, displayName: choice.model }] : [];
      for (const m of [...fromTable.map((e) => ({ id: e.model, displayName: e.label })), ...fromApi, ...current]) {
        const key = `${p.id}/${m.id}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ provider: p.id, model: m.id, label: m.displayName || m.id, entry: findPrice(table, settings, p.id, m.id) });
      }
    }
    return out;
  }, [providers, table, listed, settings, choice.provider, choice.model]);

  useEffect(() => {
    let alive = true;
    const a = settings.providers.estimate;
    void Promise.all(
      baseRows.map(async (r) => {
        if (!r.entry) return [`${r.provider}/${r.model}`, null] as const;
        const est: HourlyEstimate = await backend().invoke("estimate_cost", {
          asr: slot === "asr" ? r.entry : null,
          detect: slot === "detect" ? r.entry : null,
          hints: slot === "hints" ? r.entry : null,
          assumptions: a,
        });
        return [`${r.provider}/${r.model}`, est[slot]] as const;
      }),
    ).then((pairs) => alive && setEstimates(Object.fromEntries(pairs)));
    return () => {
      alive = false;
    };
  }, [baseRows, settings.providers.estimate, slot]);

  const isSelected = (r: Row) => choice.provider === r.provider && choice.model === r.model;
  const rows: Row[] = baseRows
    .map((r) => ({
      ...r,
      hourly: estimates[`${r.provider}/${r.model}`] ?? null,
      latencyMs: latencyFor(probe, slot, r.provider, r.model),
    }))
    .sort((a, b) => (a.hourly ?? Number.POSITIVE_INFINITY) - (b.hourly ?? Number.POSITIVE_INFINITY));
  const cheapest = rows[best(rows, "hourly")];
  const fastest = rows[best(rows, "latencyMs")];
  const visible = showAll ? rows : featured(rows, isSelected);
  const hidden = rows.length - visible.length;
  const timed = rows.some((r) => r.latencyMs !== null);

  const select = (r: Row) =>
    update((s) => {
      s.providers[slot] = { ...s.providers[slot], provider: r.provider, model: r.model };
      return s;
    });

  return (
    <div className="model-list" data-timed={timed} role="radiogroup" aria-label={t(`providers.${slot}Title`)}>
      {rows.length > 0 && (
        <div className="model-head" aria-hidden>
          <span />
          <span>{t("providers.colModel")}</span>
          <span>{t("providers.colPrice")}</span>
          <span className="num">{t("providers.colEstimate")}</span>
          {timed && <span className="num">{t("providers.colLatency")}</span>}
        </div>
      )}
      {visible.map((r) => {
        const key = `${r.provider}/${r.model}`;
        const meta = PROVIDERS.find((p) => p.id === r.provider);
        return (
          <label key={key} className="model-row" data-selected={isSelected(r)}>
            <input
              type="radio"
              name={`model-${slot}`}
              checked={isSelected(r)}
              onChange={() => void select(r)}
              aria-label={t("providers.chooseModel", { model: r.label })}
            />
            <span className="model-name">
              <span className="model-label">{r.label}</span>
              {!namesProvider(r.label, providerName(r.provider)) && <span className="muted">{providerName(r.provider)}</span>}
              {r === cheapest && <span className="tag">{t("providers.cheapest")}</span>}
              {r === fastest && <span className="tag">{t("providers.fastest")}</span>}
            </span>
            <span className="model-price" title={r.entry?.note ?? undefined}>
              {r.entry ? <UnitPrice entry={r.entry} /> : <NoPrice meta={meta} onEdit={() => setEditing(key)} />}
            </span>
            <span className="num model-est">{r.hourly !== null ? usd(r.hourly) : "—"}</span>
            {timed && <span className="num muted">{r.latencyMs !== null ? `${millis(r.latencyMs)} ms` : "—"}</span>}
          </label>
        );
      })}
      {!rows.length && <p className="muted model-empty">{t("providers.noModels")}</p>}
      {(hidden > 0 || showAll) && (
        <button className="btn-link model-more" onClick={() => setShowAll(!showAll)}>
          {showAll ? t("providers.showFewer") : t("providers.showAll", { count: hidden })}
        </button>
      )}
      {editing && (
        <PriceEditor
          slot={slot}
          provider={editing.split("/")[0]}
          model={editing.slice(editing.indexOf("/") + 1)}
          onCancel={() => setEditing(null)}
          onSave={(entry) =>
            void update((s) => {
              s.providers.pricingOverrides = [
                ...s.providers.pricingOverrides.filter((e) => !(e.provider === entry.provider && e.model === entry.model)),
                entry,
              ];
              return s;
            }).then(() => setEditing(null))
          }
        />
      )}
    </div>
  );
}

/** "$0.12 / audio hour" or "In $0.10 · Out $0.50 / 1M tokens". */
function UnitPrice({ entry }: { entry: PriceEntry }) {
  const { t } = useTranslation();
  if (entry.audio !== undefined) {
    return <>{t("providers.audioPrice", { price: price(audioPerHour(entry)) })}</>;
  }
  return (
    <>
      {t("providers.tokenPrice", {
        input: price(tokensPerMillion(entry.input, entry.unit)),
        output: price(tokensPerMillion(entry.output, entry.unit)),
      })}
    </>
  );
}

/** No published figure: point to the provider's own price page, or let the user enter one. */
function NoPrice({ meta, onEdit }: { meta: ProviderMeta | undefined; onEdit: () => void }) {
  const { t } = useTranslation();
  if (meta?.pricingUrl) {
    const url = meta.pricingUrl;
    return (
      <button
        className="btn-link"
        onClick={(e) => {
          e.preventDefault();
          void backend().invoke("open_url", { url });
        }}
      >
        {t("providers.officialPricing")} ↗
      </button>
    );
  }
  return (
    <button
      className="btn-link"
      onClick={(e) => {
        e.preventDefault();
        onEdit();
      }}
    >
      {t("providers.enterPrice")}
    </button>
  );
}

export function PriceEditor({
  slot,
  provider,
  model,
  initial,
  onSave,
  onCancel,
}: {
  slot: ModelSlot;
  provider: string;
  model: string;
  initial?: PriceEntry | null;
  onSave: (e: PriceEntry) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [a, setA] = useState(String((slot === "asr" ? initial?.audio : initial?.input) ?? ""));
  const [b, setB] = useState(String(initial?.output ?? ""));
  const num = (s: string) => (s.trim() === "" ? undefined : Number(s));
  return (
    <div className="note" role="group" aria-label={t("providers.setPriceFor", { model })}>
      <div className="row">
        <strong>{t("providers.setPriceFor", { model })}</strong>
      </div>
      <div className="row" style={{ marginTop: 8 }}>
        {slot === "asr" ? (
          <label className="row">
            {t("providers.perHourUsd")}
            <input type="number" min="0" step="0.0001" value={a} onChange={(e) => setA(e.target.value)} />
          </label>
        ) : (
          <>
            <label className="row">
              {t("providers.inputPerMillion")}
              <input type="number" min="0" step="0.0001" value={a} onChange={(e) => setA(e.target.value)} />
            </label>
            <label className="row">
              {t("providers.outputPerMillion")}
              <input type="number" min="0" step="0.0001" value={b} onChange={(e) => setB(e.target.value)} />
            </label>
          </>
        )}
        <button
          className="btn btn-primary btn-small"
          onClick={() =>
            onSave({
              provider,
              model,
              label: model,
              unit: slot === "asr" ? "per_audio_hour" : "per_million_tokens",
              ...(slot === "asr" ? { audio: num(a) } : { input: num(a), output: num(b) }),
              source: "user",
              verified: new Date().toISOString().slice(0, 10),
            })
          }
        >
          {t("common.save")}
        </button>
        <button className="btn btn-small" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}
