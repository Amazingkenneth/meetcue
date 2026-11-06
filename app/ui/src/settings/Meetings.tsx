import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { MeetingRecord } from "../bindings/MeetingRecord";
import type { MeetingSummary } from "../bindings/MeetingSummary";
import type { Settings } from "../bindings/Settings";
import { Field, Section, Switch } from "../design/components";
import { useAppEvents } from "../state/hooks";
import { boldSegments, clock, usd } from "../util/format";

type Update = (f: (s: Settings) => Settings) => Promise<void>;

/** The meeting named in the window's address (`#meetings-<id>`), if any. */
function idFromHash(): string | null {
  const hash = window.location.hash.replace("#", "");
  return hash.startsWith("meetings-") ? hash.slice("meetings-".length) || null : null;
}

function when(iso: string, lang: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(lang, { dateStyle: "medium", timeStyle: "short" });
}

export function MeetingsTab({ settings, update }: { settings: Settings; update: Update }) {
  const { t, i18n } = useTranslation();
  const [list, setList] = useState<MeetingSummary[] | null>(null);
  const [open, setOpen] = useState<string | null>(idFromHash);
  const [confirmAll, setConfirmAll] = useState(false);
  const reload = useCallback(() => {
    void backend()
      .invoke("list_meetings")
      .then(setList)
      .catch(() => setList([]));
  }, []);
  useEffect(reload, [reload]);
  useAppEvents((e) => {
    if (e.type === "meetingSaved") reload();
  });
  useEffect(() => {
    const onHash = () => setOpen(idFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  const h = settings.history;
  const set = (f: (s: Settings["history"]) => void) =>
    void update((s) => {
      f(s.history);
      return s;
    });
  if (open) return <MeetingRecap id={open} onBack={() => setOpen(null)} onDeleted={() => (setOpen(null), reload())} />;
  return (
    <>
      <h2 className="page">{t("settings.tab.meetings")}</h2>
      <Section lead={t("meetings.lead")}>
        <Switch
          checked={h.saveMeetings}
          label={t("meetings.save")}
          description={t("meetings.saveHint")}
          onChange={(v) => set((x) => (x.saveMeetings = v))}
        />
        <Switch
          checked={h.saveTranscript}
          disabled={!h.saveMeetings}
          label={t("meetings.saveTranscript")}
          description={t("meetings.saveTranscriptHint")}
          onChange={(v) => set((x) => (x.saveTranscript = v))}
        />
        <Field label={t("meetings.keep")} htmlFor="keep-days" hint={t("meetings.keepHint")}>
          <select id="keep-days" value={h.keepDays} onChange={(e) => set((x) => (x.keepDays = Number(e.target.value)))}>
            {[7, 30, 90, 365, 0].map((d) => (
              <option key={d} value={d}>
                {d === 0 ? t("meetings.keepForever") : t("meetings.keepDays", { count: d })}
              </option>
            ))}
          </select>
        </Field>
      </Section>
      <Section title={t("meetings.listTitle")}>
        {list === null ? (
          <p className="muted">{t("common.loading")}</p>
        ) : list.length === 0 ? (
          <p className="muted">{t("meetings.empty")}</p>
        ) : (
          <>
            <div className="table-wrap">
              <table className="data">
                <thead>
                  <tr>
                    <th>{t("meetings.when")}</th>
                    <th>{t("meetings.firstQuestion")}</th>
                    <th className="num">{t("meetings.questions")}</th>
                    <th className="num">{t("meetings.covered")}</th>
                    <th className="num">{t("meetings.duration")}</th>
                    <th className="num">{t("usage.cost")}</th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((m) => (
                    <tr key={m.id}>
                      <td>
                        <button className="btn-link" onClick={() => (window.location.hash = `meetings-${m.id}`)}>
                          {when(m.startedAt, i18n.language)}
                        </button>
                      </td>
                      <td>{m.firstQuestion || "—"}</td>
                      <td className="num">{m.questions}</td>
                      <td className="num">{m.pointsTotal ? `${m.pointsDone}/${m.pointsTotal}` : "—"}</td>
                      <td className="num mono">{clock(m.durationMs)}</td>
                      <td className="num">{usd(m.costMicros)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="row" style={{ marginTop: 8 }}>
              <span className="spacer" />
              {confirmAll ? (
                <>
                  <span className="muted">{t("meetings.deleteAllConfirm")}</span>
                  <button
                    className="btn btn-small btn-danger"
                    onClick={() =>
                      void backend()
                        .invoke("delete_meeting", { id: "" })
                        .then(() => (setConfirmAll(false), reload()))
                    }
                  >
                    {t("meetings.deleteAll")}
                  </button>
                  <button className="btn btn-small" onClick={() => setConfirmAll(false)}>
                    {t("common.cancel")}
                  </button>
                </>
              ) : (
                <button className="btn btn-small" onClick={() => setConfirmAll(true)}>
                  {t("meetings.deleteAll")}
                </button>
              )}
            </div>
          </>
        )}
      </Section>
    </>
  );
}

/** One meeting: every question with its points (ticked or not) and sample answer. */
export function MeetingRecap({ id, onBack, onDeleted }: { id: string; onBack: () => void; onDeleted: () => void }) {
  const { t, i18n } = useTranslation();
  const [m, setM] = useState<MeetingRecord | null>(null);
  const [failed, setFailed] = useState(false);
  const [exported, setExported] = useState<string | null>(null);
  useEffect(() => {
    setM(null);
    setFailed(false);
    void backend()
      .invoke("get_meeting", { id })
      .then(setM)
      .catch(() => setFailed(true));
  }, [id]);
  const back = () => {
    window.location.hash = "meetings";
    onBack();
  };
  if (failed)
    return (
      <>
        <button className="btn-link" onClick={back}>
          ← {t("meetings.back")}
        </button>
        <p className="muted">{t("meetings.missing")}</p>
      </>
    );
  if (!m) return <p className="muted">{t("common.loading")}</p>;
  const points = m.questions.flatMap((q) => q.points);
  const missed = points.filter((p) => !p.done).length;
  return (
    <div className="meeting-recap">
      <button className="btn-link" onClick={back}>
        ← {t("meetings.back")}
      </button>
      <h2 className="page">{t("meetings.recapTitle", { when: when(m.startedAt, i18n.language) })}</h2>
      <p className="muted">
        {t("meetings.recapFacts", {
          duration: clock(m.durationMs),
          n: m.questions.length,
          done: points.length - missed,
          total: points.length,
          cost: usd(m.costMicros),
        })}
      </p>
      <div className="row" style={{ margin: "8px 0 16px" }}>
        <button
          className="btn btn-small"
          onClick={() =>
            void backend()
              .invoke("export_meeting", { id: m.id })
              .then((p) => p && setExported(p))
          }
        >
          {t("meetings.export")}
        </button>
        <button
          className="btn btn-small"
          onClick={() =>
            void backend()
              .invoke("delete_meeting", { id: m.id })
              .then(() => {
                window.location.hash = "meetings";
                onDeleted();
              })
          }
        >
          {t("meetings.delete")}
        </button>
        {exported && (
          <span className="muted" role="status">
            {t("meetings.exported", { path: exported })}
          </span>
        )}
      </div>
      {m.questions.length === 0 && <p className="muted">{t("meetings.noQuestions")}</p>}
      {m.questions.map((q, i) => (
        <Section key={i} title={`${i + 1}. ${q.short || q.text}`}>
          <p>
            <span className="mono muted">{clock(q.atMs)}</span>{" "}
            {q.speaker ? <span className="muted">{t("overlay.speakerN", { n: q.speaker })} · </span> : null}
            {q.text}
          </p>
          {q.points.length > 0 && (
            <ul className="recap-points">
              {q.points.map((p, j) => (
                <li key={j} data-done={p.done}>
                  <span aria-label={p.done ? t("overlay.doneLabel") : t("meetings.notCovered")}>{p.done ? "✓" : "○"}</span>{" "}
                  <strong>{p.head}</strong> {p.rest}
                </li>
              ))}
            </ul>
          )}
          {q.answer && (
            <details>
              <summary>{t("meetings.sampleAnswer")}</summary>
              <p>{boldSegments(q.answer).map((s, k) => (s.bold ? <strong key={k}>{s.text}</strong> : <span key={k}>{s.text}</span>))}</p>
            </details>
          )}
        </Section>
      ))}
      {m.transcript.length > 0 && (
        <Section title={t("meetings.transcript")}>
          {m.transcript.map((l, i) => (
            <p key={i}>
              <span className="mono muted">{clock(l.t0Ms)}</span> <strong>{l.channel === "me" ? t("overlay.me") : t("overlay.them")}</strong> {l.text}
            </p>
          ))}
        </Section>
      )}
    </div>
  );
}
