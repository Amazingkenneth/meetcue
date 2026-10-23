import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { PracticeReport } from "../bindings/PracticeReport";
import type { PracticeReportSummary } from "../bindings/PracticeReportSummary";
import type { QuestionReview } from "../bindings/QuestionReview";
import { Section } from "../design/components";
import { useAppEvents, useSettings } from "../state/hooks";
import { seconds } from "../util/format";
import "./practice.css";

function reportIdFromUrl(): string | null {
  const id = decodeURIComponent(window.location.hash.replace(/^#/, ""));
  return id || null;
}

function percent(v: number | null | undefined): string {
  return v === null || v === undefined ? "—" : `${Math.round(v * 100)}%`;
}

export function Review() {
  const { t } = useTranslation();
  useSettings(); // applies the interface language
  const [report, setReport] = useState<PracticeReport | null>(null);
  const [history, setHistory] = useState<PracticeReportSummary[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [id, setId] = useState<string | null>(reportIdFromUrl);
  const [exported, setExported] = useState<string | null>(null);

  useEffect(() => {
    const onHash = () => setId(reportIdFromUrl());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  useAppEvents((e) => {
    if (e.type === "practiceFinished") setId(e.reportId);
  });
  useEffect(() => {
    let alive = true;
    void (async () => {
      const list = await backend().invoke("practice_reports").catch(() => []);
      const target = id ?? list[0]?.id ?? null;
      const r = target ? await backend().invoke("practice_report", { id: target }).catch(() => null) : null;
      if (!alive) return;
      setHistory(list);
      setReport(r);
      setLoaded(true);
      setExported(null);
    })();
    return () => {
      alive = false;
    };
  }, [id]);

  if (!loaded) return <p className="app-body muted">{t("common.loading")}</p>;
  if (!report) {
    return (
      <div className="app">
        <header className="app-header">
          <h1>{t("review.title")}</h1>
        </header>
        <p className="app-body muted">{t("review.empty")}</p>
      </div>
    );
  }
  const s = report.summary;
  return (
    <div className="app review">
      <header className="app-header review-header">
        <div>
          <h1>{report.title}</h1>
          <p>
            {new Date(report.startedAt).toLocaleString()} · {t("review.minutes", { value: Math.max(1, Math.round(report.durationMs / 60_000)) })} ·{" "}
            {t(report.playback === "realPipeline" ? "practice.playbackReal" : "practice.playbackSimulated")} ·{" "}
            {t(
              {
                full: "practice.levelFull",
                headsOnly: "practice.levelHeads",
                delayed: "practice.levelDelayed",
                off: "practice.levelOff",
              }[report.hintLevel],
            )}
          </p>
        </div>
        <button
          className="btn"
          onClick={async () => {
            const path = await backend().invoke("export_report", { id: report.id });
            if (path) setExported(path);
          }}
        >
          {t("review.export")}
        </button>
      </header>
      <div className="app-body">
        {exported && (
          <p className="note" role="status">
            {t("review.exported", { path: exported })}
          </p>
        )}
        {report.simulated && <p className="note note-warn">{t("review.simulatedNote")}</p>}

        <Section title={t("review.summary")}>
          <div className="review-tiles">
            <Tile label={t("review.answered")} value={`${s.answered} / ${s.questions}`} />
            <Tile
              label={t("review.avgStart")}
              value={s.avgStartLatencyMs !== null ? t("review.seconds", { value: seconds(s.avgStartLatencyMs) }) : "—"}
            />
            <Tile label={t("review.avgCoverage")} value={percent(s.avgCoverage)} />
            <Tile label={t("review.fillers")} value={String(s.totalFillers)} />
            <Tile label={t("review.pace")} value={s.avgPace !== null ? String(Math.round(s.avgPace)) : "—"} />
          </div>
        </Section>

        <Section title={t("review.questions")}>
          {report.questions.map((q, i) => (
            <QuestionCard key={i} n={i + 1} q={q} />
          ))}
        </Section>

        {history.length > 1 && (
          <Section title={t("review.trend")} lead={t("review.trendLead")}>
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th scope="col">{t("review.colDate")}</th>
                    <th scope="col">{t("review.colPractice")}</th>
                    <th scope="col">{t("review.colQuestions")}</th>
                    <th scope="col">{t("review.avgStart")}</th>
                    <th scope="col">{t("review.avgCoverage")}</th>
                  </tr>
                </thead>
                <tbody>
                  {history.map((h) => (
                    <tr key={h.id} aria-current={h.id === report.id ? "true" : undefined}>
                      <td>
                        <button className="btn-link" onClick={() => (window.location.hash = h.id)}>
                          {new Date(h.startedAt).toLocaleDateString()}
                        </button>
                        {h.id === report.id && <span className="badge">{t("review.thisSession")}</span>}
                      </td>
                      <td>{h.title}</td>
                      <td>{h.questions}</td>
                      <td>
                        <TrendBar value={h.avgStartLatencyMs} max={Math.max(...history.map((x) => x.avgStartLatencyMs ?? 0), 1)} />
                        {h.avgStartLatencyMs !== null ? t("review.seconds", { value: seconds(h.avgStartLatencyMs) }) : "—"}
                      </td>
                      <td>
                        <TrendBar value={h.avgCoverage} max={1} />
                        {percent(h.avgCoverage)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Section>
        )}
      </div>
    </div>
  );
}

function Tile({ label, value }: { label: string; value: string }) {
  return (
    <div className="review-tile">
      <span className="muted">{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function TrendBar({ value, max }: { value: number | null; max: number }) {
  const w = value === null ? 0 : Math.max(2, Math.round((value / max) * 60));
  return <span className="trend-bar" style={{ width: w }} aria-hidden />;
}

function QuestionCard({ n, q }: { n: number; q: QuestionReview }) {
  const { t } = useTranslation();
  return (
    <article className="review-q" data-skipped={q.skipped}>
      <h3>
        {n}. {q.question}
      </h3>
      {q.skipped ? (
        <p className="muted">{t("review.skipped")}</p>
      ) : (
        <>
          <dl className="review-facts">
            {q.startLatencyMs !== null && (
              <div>
                <dt>{t("review.timeToStart")}</dt>
                <dd>{t("review.seconds", { value: seconds(q.startLatencyMs) })}</dd>
              </div>
            )}
            {q.answerMs !== null && (
              <div>
                <dt>{t("review.answerLength")}</dt>
                <dd>{t("review.seconds", { value: seconds(q.answerMs) })}</dd>
              </div>
            )}
            {q.pointsTotal > 0 && (
              <div>
                <dt>{t("review.coverage")}</dt>
                <dd>
                  {q.pointsDone}/{q.pointsTotal}
                </dd>
              </div>
            )}
            {q.pace !== null && (
              <div>
                <dt>{t("review.pace")}</dt>
                <dd>
                  {Math.round(q.pace)} {t(q.paceUnit === "cpm" ? "review.cpm" : "review.wpm")}
                </dd>
              </div>
            )}
            {q.fillers.length > 0 && (
              <div>
                <dt>{t("review.fillers")}</dt>
                <dd>{q.fillers.map((f) => `${f.word} ×${f.count}`).join(", ")}</dd>
              </div>
            )}
          </dl>
          <h4>{t("review.yourAnswer")}</h4>
          <blockquote>{q.answer.trim() ? q.answer : <span className="muted">{t("review.noTranscript")}</span>}</blockquote>
          {q.reference && (
            <>
              <h4>{t("review.reference")}</h4>
              <blockquote>{q.reference}</blockquote>
            </>
          )}
          {q.feedback.length > 0 && (
            <>
              <h4>{t("review.feedback")}</h4>
              <ol className="review-feedback">
                {q.feedback.map((f, i) => (
                  <li key={i}>{f}</li>
                ))}
              </ol>
            </>
          )}
          {q.followUp && <p className="muted">{t("review.followUp", { question: q.followUp })}</p>}
        </>
      )}
    </article>
  );
}
