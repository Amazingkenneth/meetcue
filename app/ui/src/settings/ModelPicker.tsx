import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { ModelSlot } from "../backend/types";
import type { HourlyEstimate } from "../bindings/HourlyEstimate";
import type { PriceEntry } from "../bindings/PriceEntry";
import type { PriceTable } from "../bindings/PriceTable";
import type { ProbeReport } from "../bindings/ProbeReport";
import type { Settings } from "../bindings/Settings";
import { Badge } from "../design/components";
import { date, millis, price, usd } from "../util/format";
import { audioPerHour, providerName, providersFor, tokensPerMillion } from "./providers";

export interface Row {
  provider: string;
  model: string;
  label: string;
  entry: PriceEntry | null;
  hourly: number | null;
  latencyMs: number | null;
}

type Sort = "price" | "latency";
type Highlight = "cheapest" | "fastest" | "balanced" | null;

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

/** Index of the suggested row for a highlight mode. */
export function suggest(rows: Row[], mode: Highlight): number {
  if (!mode || !rows.length) return -1;
  const priced = rows.map((r, i) => ({ r, i })).filter((x) => x.r.hourly !== null);
  const timed = rows.map((r, i) => ({ r, i })).filter((x) => x.r.latencyMs !== null);
  if (mode === "cheapest") {
    return priced.length ? priced.reduce((a, b) => (b.r.hourly! < a.r.hourly! ? b : a)).i : -1;
  }
  if (mode === "fastest") {
    return timed.length ? timed.reduce((a, b) => (b.r.latencyMs! < a.r.latencyMs! ? b : a)).i : -1;
  }
  const both = rows.map((r, i) => ({ r, i })).filter((x) => x.r.hourly !== null && x.r.latencyMs !== null);
  if (!both.length) return priced.length ? suggest(rows, "cheapest") : -1;
  const minH = Math.max(1, Math.min(...both.map((x) => x.r.hourly!)));
  const minL = Math.max(1, Math.min(...both.map((x) => x.r.latencyMs!)));
  return both.reduce((a, b) => (b.r.hourly! / minH + b.r.latencyMs! / minL < a.r.hourly! / minH + a.r.latencyMs! / minL ? b : a)).i;
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
  update: (f: (s: Settings) => Settings) => Promise<void>;
  table: PriceTable | null;
  probe: ProbeReport | null;
  credentials: Record<string, boolean>;
}) {
  const { t } = useTranslation();
  const [listed, setListed] = useState<Record<string, { id: string; displayName: string }[]>>({});
  const [estimates, setEstimates] = useState<Record<string, number | null>>({});
  const [sort, setSort] = useState<Sort>("price");
  const [highlight, setHighlight] = useState<Highlight>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const providers = useMemo(() => providersFor(slot).filter((p) => p.available), [slot]);
  const choice = settings.providers[slot];

  // Fetch model lists from providers whose key is configured.
  useEffect(() => {
    let alive = true;
    for (const p of providers) {
      if (!credentials[p.id] && !p.keyOptional) continue;
      void backend()
        .invoke("list_models", { provider: p.id })
        .then((models) => alive && setListed((m) => ({ ...m, [p.id]: models })))
        .catch(() => undefined);
    }
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [slot, JSON.stringify(credentials)]);

  const baseRows = useMemo(() => {
    const out: Omit<Row, "hourly" | "latencyMs">[] = [];
    const seen = new Set<string>();
    for (const p of providers) {
      const fromTable = (table?.entries ?? []).filter((e) => e.provider === p.id);
      const fromApi = listed[p.id] ?? [];
      for (const m of [...fromTable.map((e) => ({ id: e.model, displayName: e.label })), ...fromApi]) {
        const key = `${p.id}/${m.id}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ provider: p.id, model: m.id, label: m.displayName || m.id, entry: findPrice(table, settings, p.id, m.id) });
      }
    }
    return out;
  }, [providers, table, listed, settings]);

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

  const rows: Row[] = baseRows
    .map((r) => ({
      ...r,
      hourly: estimates[`${r.provider}/${r.model}`] ?? null,
      latencyMs: latencyFor(probe, slot, r.provider, r.model),
    }))
    .sort((a, b) => {
      const key = sort === "price" ? "hourly" : "latencyMs";
      return (a[key] ?? Number.POSITIVE_INFINITY) - (b[key] ?? Number.POSITIVE_INFINITY);
    });
  const suggested = suggest(rows, highlight);

  const select = (r: Row) =>
    update((s) => {
      s.providers[slot] = { ...s.providers[slot], provider: r.provider, model: r.model };
      return s;
    });

  return (
    <div>
      <div className="row" style={{ margin: "4px 0 8px" }}>
        <span className="muted">{t("providers.highlight")}</span>
        <span className="btn-group" role="group" aria-label={t("providers.highlight")}>
          {(["cheapest", "fastest", "balanced"] as const).map((h) => (
            <button
              key={h}
              className="btn btn-small"
              aria-pressed={highlight === h}
              onClick={() => setHighlight(highlight === h ? null : h)}
            >
              {t(`providers.${h}`)}
            </button>
          ))}
        </span>
        <span className="spacer" />
        <span className="muted">{t("providers.sortBy")}</span>
        <span className="btn-group" role="group" aria-label={t("providers.sortBy")}>
          <button className="btn btn-small" aria-pressed={sort === "price"} onClick={() => setSort("price")}>
            {t("providers.sortPrice")}
          </button>
          <button className="btn btn-small" aria-pressed={sort === "latency"} onClick={() => setSort("latency")}>
            {t("providers.sortLatency")}
          </button>
        </span>
      </div>
      <div className="table-wrap">
        <table className="data">
          <thead>
            <tr>
              <th>{t("providers.colModel")}</th>
              {slot === "asr" ? (
                <th className="num">{t("providers.colPerHour")}</th>
              ) : (
                <>
                  <th className="num">{t("providers.colInput")}</th>
                  <th className="num">{t("providers.colOutput")}</th>
                </>
              )}
              <th className="num">{t("providers.colEstimate")}</th>
              <th className="num">{t("providers.colLatency")}</th>
              <th>{t("providers.colChoose")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const selected = choice.provider === r.provider && choice.model === r.model;
              const key = `${r.provider}/${r.model}`;
              return (
                <tr key={key} data-selected={selected} data-suggested={i === suggested}>
                  <td>
                    <div>{r.label}</div>
                    <div className="muted" style={{ fontSize: 12 }}>
                      {providerName(r.provider)}
                      {i === suggested && (
                        <>
                          {" "}
                          <Badge tone="warn">{t("providers.suggested")}</Badge>
                        </>
                      )}
                    </div>
                  </td>
                  {slot === "asr" ? (
                    <td className="num">{r.entry ? price(audioPerHour(r.entry)) : <UnknownPrice onEdit={() => setEditing(key)} />}</td>
                  ) : (
                    <>
                      <td className="num">
                        {r.entry ? price(tokensPerMillion(r.entry.input, r.entry.unit)) : <UnknownPrice onEdit={() => setEditing(key)} />}
                      </td>
                      <td className="num">{r.entry ? price(tokensPerMillion(r.entry.output, r.entry.unit)) : ""}</td>
                    </>
                  )}
                  <td className="num" title={t("providers.estimateFormula")}>
                    {r.hourly !== null ? usd(r.hourly) : "—"}
                  </td>
                  <td className="num">{r.latencyMs !== null ? `${millis(r.latencyMs)} ms` : <span className="muted">{t("providers.notMeasured")}</span>}</td>
                  <td>
                    <label className="row">
                      <input
                        type="radio"
                        name={`model-${slot}`}
                        checked={selected}
                        onChange={() => void select(r)}
                        aria-label={t("providers.chooseModel", { model: r.label })}
                      />
                      {selected ? t("providers.selected") : ""}
                    </label>
                  </td>
                </tr>
              );
            })}
            {!rows.length && (
              <tr>
                <td colSpan={6} className="muted">
                  {t("providers.noModels")}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
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
      <p className="muted" style={{ fontSize: 12 }}>
        {t("providers.priceNote", { date: table ? date(table.updated) : "—" })}
      </p>
    </div>
  );
}

function UnknownPrice({ onEdit }: { onEdit: () => void }) {
  const { t } = useTranslation();
  return (
    <button className="btn-link" onClick={onEdit}>
      {t("providers.priceUnknown")}
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
    <div className="note" role="group" aria-label={t("providers.setPrice")}>
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
