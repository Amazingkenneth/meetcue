import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { ComboEstimate } from "../bindings/ComboEstimate";
import type { ProbeReport } from "../bindings/ProbeReport";
import type { ProbeTarget } from "../bindings/ProbeTarget";
import type { Settings } from "../bindings/Settings";
import type { Stat } from "../bindings/Stat";
import { useAppEvents } from "../state/hooks";
import { date, millis, usd } from "../util/format";
import { errorTitle } from "../util/errors";
import { PROVIDERS, providerName } from "./providers";

type Order = "fastest" | "cheapest" | "balanced";

function ms(s: Stat | null): string {
  return s ? `${millis(s.medianMs)} / ${millis(s.maxMs)}` : "—";
}

export function sortCombos(combos: ComboEstimate[], order: Order): ComboEstimate[] {
  const v = [...combos];
  const cost = (c: ComboEstimate) => c.hourlyMicros ?? Number.POSITIVE_INFINITY;
  if (order === "fastest") return v.sort((a, b) => a.endToEndMs - b.endToEndMs);
  if (order === "cheapest") return v.sort((a, b) => cost(a) - cost(b) || a.endToEndMs - b.endToEndMs);
  const minT = Math.max(1, Math.min(...v.map((c) => c.endToEndMs)));
  const minC = Math.max(1, Math.min(...v.map((c) => c.hourlyMicros ?? Number.POSITIVE_INFINITY)));
  const score = (c: ComboEstimate) => c.endToEndMs / minT + (c.hourlyMicros ?? minC * 10) / minC;
  return v.sort((a, b) => score(a) - score(b));
}

/** Targets for the probe: every available provider, with the chosen model where set. */
export function probeTargets(settings: Settings): ProbeTarget[] {
  return PROVIDERS.filter((p) => p.available && (p.id !== "openai-compatible" || settings.providers.openaiBaseUrl))
    .map((p) => {
      const slot = p.kind === "asr" ? settings.providers.asr : settings.providers.hints;
      return {
        provider: p.id,
        kind: p.kind,
        model: slot.provider === p.id ? (slot.model ?? undefined) : undefined,
        hosts: [],
      };
    });
}

export function ProbePanel({
  settings,
  credentials,
  update,
  onReport,
}: {
  settings: Settings;
  credentials: Record<string, boolean>;
  update: (f: (s: Settings) => Settings) => Promise<void>;
  onReport?: (r: ProbeReport) => void;
}) {
  const { t } = useTranslation();
  const [running, setRunning] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [networkOnly, setNetworkOnly] = useState(false);
  const [label, setLabel] = useState("");
  const [progress, setProgress] = useState<{ done: number; total: number; label: string } | null>(null);
  const [history, setHistory] = useState<ProbeReport[]>([]);
  const [order, setOrder] = useState<Order>("balanced");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void backend()
      .invoke("probe_history")
      .then((h) => {
        setHistory(h);
        if (h[0]) onReport?.(h[0]);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useAppEvents((e) => {
    if (e.type === "probeProgress") setProgress({ done: e.done, total: e.total, label: e.label });
  });

  const report = history[0] ?? null;
  const combos = useMemo(() => (report ? sortCombos(report.combos, order) : []), [report, order]);
  const anyKey = Object.values(credentials).some(Boolean);

  const run = async () => {
    setConfirming(false);
    setRunning(true);
    setError(null);
    try {
      const r = await backend().invoke("run_probe", {
        request: { targets: probeTargets(settings), label: label.trim() || undefined, networkOnly: networkOnly || !anyKey },
      });
      setHistory((h) => [r, ...h]);
      onReport?.(r);
    } catch (e) {
      setError(String((e as { code?: string })?.code ?? e));
    } finally {
      setRunning(false);
      setProgress(null);
    }
  };

  const apply = (c: ComboEstimate) =>
    update((s) => {
      s.providers.asr = { provider: c.asrProvider, model: c.asrModel ?? s.providers.asr.model, endpoint: { mode: "host", host: c.asrHost } };
      s.providers.hints = { ...s.providers.hints, provider: c.llmProvider, model: c.llmModel ?? s.providers.hints.model };
      if (!s.providers.detect.provider) s.providers.detect = { ...s.providers.hints };
      return s;
    });

  return (
    <div id="probe">
      <p className="lead">{t("probe.lead")}</p>
      <div className="row" style={{ marginBottom: 8 }}>
        <label className="row">
          <input type="checkbox" checked={networkOnly || !anyKey} disabled={!anyKey} onChange={(e) => setNetworkOnly(e.target.checked)} />
          {t("probe.networkOnly")}
        </label>
        <label className="row">
          {t("probe.label")}
          <input type="text" value={label} placeholder={t("probe.labelPlaceholder")} onChange={(e) => setLabel(e.target.value)} />
        </label>
        <span className="spacer" />
        <button className="btn btn-primary" disabled={running} onClick={() => (networkOnly || !anyKey ? void run() : setConfirming(true))}>
          {running ? t("probe.running") : t("probe.run")}
        </button>
      </div>
      {confirming && (
        <div className="note note-warn" role="alertdialog" aria-label={t("probe.costTitle")}>
          <strong>{t("probe.costTitle")}</strong>
          <p style={{ margin: "4px 0 8px" }}>{t("probe.costBody")}</p>
          <div className="row">
            <button className="btn btn-primary btn-small" onClick={() => void run()}>
              {t("probe.costConfirm")}
            </button>
            <button className="btn btn-small" onClick={() => setConfirming(false)}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}
      {!anyKey && <p className="note">{t("probe.noKeys")}</p>}
      {progress && (
        <p className="muted" role="status">
          {t("probe.progress", { done: progress.done, total: progress.total })} {progress.label && `· ${providerName(progress.label)}`}
        </p>
      )}
      {error && <p className="note note-err">{errorTitle(error)} · {error}</p>}
      {report && (
        <>
          <h3 style={{ fontSize: 14, margin: "16px 0 6px" }}>
            {t("probe.results", { date: date(report.at) })}
            {report.label ? ` · ${report.label}` : ""}
          </h3>
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th>{t("probe.colEndpoint")}</th>
                  <th className="num">{t("probe.colTls")}</th>
                  <th className="num">{t("probe.colFirst")}</th>
                  <th className="num">{t("probe.colFinal")}</th>
                  <th className="num">{t("providers.colEstimate")}</th>
                </tr>
              </thead>
              <tbody>
                {report.results.map((r) => (
                  <tr key={`${r.provider}/${r.host}/${r.model ?? ""}`}>
                    <td>
                      <div className="mono">{r.host}</div>
                      <div className="muted" style={{ fontSize: 12 }}>
                        {providerName(r.provider)}
                        {r.model ? ` · ${r.model}` : ""}
                        {r.error ? ` · ${errorTitle(r.error)} (${r.error})` : ""}
                      </div>
                    </td>
                    <td className="num">{ms(r.tls)}</td>
                    <td className="num">{ms(r.kind === "asr" ? r.firstPartial : r.firstToken)}</td>
                    <td className="num">{ms(r.kind === "asr" ? r.finalResult : r.firstPoint)}</td>
                    <td className="num">{usd(r.hourlyMicros)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="muted" style={{ fontSize: 12 }}>
            {t("probe.legend")}
          </p>
          {combos.length > 0 && (
            <>
              <div className="row" style={{ margin: "12px 0 6px" }}>
                <h3 style={{ fontSize: 14, margin: 0 }}>{t("probe.combos")}</h3>
                <span className="spacer" />
                <span className="btn-group" role="group" aria-label={t("providers.sortBy")}>
                  {(["fastest", "cheapest", "balanced"] as const).map((o) => (
                    <button key={o} className="btn btn-small" aria-pressed={order === o} onClick={() => setOrder(o)}>
                      {t(`providers.${o}`)}
                    </button>
                  ))}
                </span>
              </div>
              <div className="table-wrap">
                <table className="data">
                  <thead>
                    <tr>
                      <th>{t("probe.colAsr")}</th>
                      <th>{t("probe.colLlm")}</th>
                      <th className="num">{t("probe.colEndToEnd")}</th>
                      <th className="num">{t("providers.colEstimate")}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {combos.map((c, i) => (
                      <tr key={i} data-suggested={i === 0}>
                        <td>
                          <div className="mono">{c.asrHost}</div>
                          <div className="muted" style={{ fontSize: 12 }}>
                            {providerName(c.asrProvider)} {c.asrModel ?? ""}
                          </div>
                        </td>
                        <td>
                          <div className="mono">{c.llmHost}</div>
                          <div className="muted" style={{ fontSize: 12 }}>
                            {providerName(c.llmProvider)} {c.llmModel ?? ""}
                          </div>
                        </td>
                        <td className="num">{millis(c.endToEndMs)} ms</td>
                        <td className="num">{usd(c.hourlyMicros)}</td>
                        <td>
                          <button className="btn btn-small" onClick={() => void apply(c)}>
                            {t("probe.apply")}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <p className="muted" style={{ fontSize: 12 }}>
                {t("probe.estimateNote")}
              </p>
            </>
          )}
        </>
      )}
    </div>
  );
}
