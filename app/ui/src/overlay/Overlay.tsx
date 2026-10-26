import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { HintLevel } from "../bindings/HintLevel";
import type { PanelMode } from "../bindings/PanelMode";
import type { Settings } from "../bindings/Settings";
import type { TranscriptLine } from "../bindings/TranscriptLine";
import {
  contextAround,
  coverageCount,
  currentId,
  medianLatency,
  neighbours,
  nextPoint,
  type QuestionView,
  type SessionErrorView,
  type SessionView,
} from "../state/session";
import { activeMs, useAppEvents, useAppInfo, useNow, useSession, useSettings, type PauseClock } from "../state/hooks";
import { latencyTip } from "../state/latency";
import { usePracticeProgress, type PracticeProgress } from "../state/practice";
import { boldSegments, clock, seconds } from "../util/format";
import { errorDocsUrl, errorFix, errorTitle } from "../util/errors";
import "./overlay.css";

const SLOW_MS = 2500;

/** How hints are shown: always fully in meetings, per the practice level in practice. */
export interface HintDisplay {
  level: HintLevel;
  delayS: number;
}

const FULL: HintDisplay = { level: "full", delayS: 0 };

export function Overlay() {
  const [settings] = useSettings();
  const { view, wallStart, pauses } = useSession();
  const info = useAppInfo();
  const [mode, setMode] = useState<PanelMode>("full");
  useEffect(() => {
    if (settings) setMode(settings.overlay.mode === "hidden" ? "full" : settings.overlay.mode);
  }, [settings]);
  useAppEvents((e) => {
    if (e.type === "panelMode" && e.mode !== "hidden") setMode(e.mode);
  });
  const practice = usePracticeProgress();
  const display: HintDisplay =
    practice && settings ? { level: settings.practice.hintLevel, delayS: settings.practice.hintDelayS } : FULL;
  const docsUrl = info?.docsUrl ?? "";
  if (mode === "mini") {
    return <MiniBar view={view} display={display} onExpand={() => void backend().invoke("set_panel_mode", { mode: "full" })} />;
  }
  return (
    <FullPanel
      view={view}
      wallStart={wallStart}
      pauses={pauses}
      settings={settings}
      demo={!!info?.demo}
      docsUrl={docsUrl}
      practice={practice}
      display={display}
    />
  );
}

function PracticeBar({ p }: { p: PracticeProgress }) {
  const { t } = useTranslation();
  const paused = p.phase === "paused";
  const control = (c: "pause" | "resume" | "skip" | "repeat") => void backend().invoke("practice_control", { control: c });
  return (
    <div className="ov-practice" role="status">
      <strong>{t("practice.progress", { index: p.index, total: p.total })}</strong>
      <span className="ov-muted">{p.nudge ? t("practice.phase.nudge") : t(`practice.phase.${p.phase}`)}</span>
      <span className="grow" />
      <button className="ov-icon" onClick={() => control("repeat")}>
        {t("practice.repeat")}
      </button>
      <button className="ov-icon" onClick={() => control(paused ? "resume" : "pause")}>
        {paused ? t("practice.resume") : t("practice.pause")}
      </button>
      <button className="ov-icon" onClick={() => control("skip")}>
        {t("practice.skip")}
      </button>
    </div>
  );
}

function FullPanel({
  view,
  wallStart,
  pauses,
  settings,
  demo,
  docsUrl,
  practice,
  display,
}: {
  view: SessionView;
  wallStart: number | null;
  pauses: PauseClock;
  settings: Settings | null;
  demo: boolean;
  docsUrl: string;
  practice: PracticeProgress | null;
  display: HintDisplay;
}) {
  const { t } = useTranslation();
  const now = useNow(1000);
  const [pinned, setPinned] = useState(true);
  // Notices from the shell (hotkey taken, device changed, …) share the error toast.
  const [notices, setNotices] = useState<SessionErrorView[]>([]);
  useAppEvents((e) => {
    if (e.type === "notice") setNotices((n) => [...n, { code: e.code, detail: e.detail, at: Date.now() }].slice(-5));
  });
  const problems = useMemo(() => [...view.errors, ...notices].sort((a, b) => a.at - b.at), [view.errors, notices]);
  const elapsed = wallStart ? clock(activeMs(now, wallStart, pauses)) : "00:00";
  const running = view.state === "running";
  const stateText =
    view.state === "running"
      ? t("overlay.listening", { time: elapsed })
      : view.state === "paused"
        ? t("overlay.paused", { time: elapsed })
        : t("overlay.idle");
  const toggleKey = settings?.hotkeys.toggleCapture ?? "Ctrl+Alt+Space";

  return (
    <div className="ov" role="application" aria-label={t("overlay.title")}>
      <div className="ov-title" data-tauri-drag-region>
        <span className="ov-dot" data-on={running} aria-hidden />
        <h1 data-tauri-drag-region>{t("overlay.title")}</h1>
        {demo && <span className="ov-demo-badge">{t("overlay.demo")}</span>}
        <span className="ov-state" data-tauri-drag-region>
          {stateText}
        </span>
        <button className="ov-icon" onClick={() => void backend().invoke("toggle_capture")} title={toggleKey}>
          {running ? t("overlay.pause") : view.state === "paused" ? t("overlay.resume") : t("overlay.start")}
        </button>
        <button
          className="ov-icon"
          aria-pressed={pinned}
          title={pinned ? t("overlay.unpin") : t("overlay.pin")}
          onClick={() => {
            const on = !pinned;
            setPinned(on);
            void backend().invoke("set_always_on_top", { on });
          }}
        >
          {t("overlay.pinShort")}
        </button>
        <button
          className="ov-icon"
          title={t("overlay.toMini")}
          onClick={() => void backend().invoke("set_panel_mode", { mode: "mini" })}
        >
          {t("overlay.miniShort")}
        </button>
      </div>

      {practice && <PracticeBar p={practice} />}

      <div className="ov-sources">
        {(["me", "remote"] as const).map((ch) => {
          const label = t(ch === "me" ? "overlay.sourceMe" : "overlay.sourceRemote");
          const offline = view.state !== "idle" && view.state !== "stopped" && view.recognition[ch] === false;
          return (
            <div key={ch} className="ov-source" data-offline={offline}>
              <span>
                {label}
                {offline && <span className="ov-offline"> · {t("overlay.reconnecting")}</span>}
              </span>
              <Meter db={view.levels[ch]} label={label} />
            </div>
          );
        })}
      </div>

      <QueueCards view={view} />

      {view.latencyWarning !== null && <SlowToast view={view} settings={settings} />}

      <ErrorToast errors={problems} docsUrl={docsUrl} />

      <ChatList view={view} toggleKey={toggleKey} display={display} />

      <Footer view={view} settings={settings} />
    </div>
  );
}

function Meter({ db, label }: { db: number; label: string }) {
  const bars = db > -20 ? 4 : db > -30 ? 3 : db > -40 ? 2 : db > -50 ? 1 : 0;
  return (
    <span className="ov-meter" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={4} aria-valuenow={bars}>
      {[1, 2, 3, 4].map((i) => (
        <span key={i} data-on={i <= bars} />
      ))}
    </span>
  );
}

function StatusText({ q }: { q: QuestionView }) {
  const { t } = useTranslation();
  const key = q.status === "answered" ? "status.answered" : q.status === "active" ? "status.answering" : "status.waiting";
  return <span className={`st st-${q.status}`}>{t(key)}</span>;
}

function QueueCards({ view }: { view: SessionView }) {
  const { t } = useTranslation();
  const { prev, current, next } = neighbours(view);
  const card = (label: string, q: QuestionView | null, isCurrent: boolean) => (
    <button
      className="ov-qcard"
      data-current={isCurrent}
      disabled={!q}
      onClick={() => q && void backend().invoke("user_action", { action: { type: "focus", id: q.id } })}
    >
      <span className="label">{label}</span>
      <span className="short">{q ? q.short : "—"}</span>
      {q ? <StatusText q={q} /> : <span className="st">&nbsp;</span>}
    </button>
  );
  return (
    <nav className="ov-queue" aria-label={t("overlay.queue")}>
      {card(t("overlay.prev"), prev, false)}
      {card(t("overlay.current"), current, true)}
      {card(t("overlay.next"), next, false)}
    </nav>
  );
}

function ChatList({ view, toggleKey, display }: { view: SessionView; toggleKey: string; display: HintDisplay }) {
  const { t } = useTranslation();
  const current = currentId(view);
  const refs = useRef(new Map<number, HTMLElement>());
  const currentPoints = current === null ? 0 : (view.questions[current]?.points.length ?? 0);
  useEffect(() => {
    if (current === null) return;
    refs.current.get(current)?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
  }, [current, currentPoints]);

  if (!view.order.length) {
    return (
      <div className="ov-chat">
        <div className="ov-empty">
          {view.state === "running" ? (
            <>
              <strong>{t("overlay.emptyTitle")}</strong>
              <p>{t("overlay.emptyBody")}</p>
            </>
          ) : (
            <>
              <p>
                {t("overlay.idleHintBefore")} <kbd>{toggleKey}</kbd> {t("overlay.idleHintAfter")}
              </p>
              <button className="ov-link" onClick={() => void backend().invoke("open_window", { view: "practice" })}>
                {t("overlay.practiceLink")}
              </button>
            </>
          )}
        </div>
      </div>
    );
  }
  return (
    <div className="ov-chat" aria-live="polite">
      {view.order.map((id) => {
        const q = view.questions[id];
        if (!q) return null;
        return (
          <ChatBlock
            key={id}
            q={q}
            view={view}
            display={display}
            isCurrent={id === current}
            ref={(el: HTMLElement | null) => {
              if (el) refs.current.set(id, el);
              else refs.current.delete(id);
            }}
          />
        );
      })}
    </div>
  );
}

/** Whether hints are hidden right now, and for how many more seconds. */
export function hintVisibility(display: HintDisplay, shownAt: number, now: number): { hidden: boolean; waitS: number } {
  if (display.level === "off") return { hidden: true, waitS: 0 };
  if (display.level === "delayed") {
    const left = display.delayS * 1000 - (now - shownAt);
    if (left > 0) return { hidden: true, waitS: Math.ceil(left / 1000) };
  }
  return { hidden: false, waitS: 0 };
}

function ChatBlock({
  q,
  view,
  display,
  isCurrent,
  ref,
}: {
  q: QuestionView;
  view: SessionView;
  display: HintDisplay;
  isCurrent: boolean;
  ref: (el: HTMLElement | null) => void;
}) {
  const { t } = useTranslation();
  const [shownAt] = useState(() => Date.now());
  const now = useNow(display.level === "delayed" ? 500 : 60_000);
  const vis = hintVisibility(display, shownAt, now);
  const headsOnly = display.level === "headsOnly";
  const [showContext, setShowContext] = useState(false);
  const [showAnswer, setShowAnswer] = useState(false);
  const { done, total } = coverageCount(q);
  const at = view.startedAt !== null ? clock(q.tMs - view.startedAt) : "";
  const context = useMemo(() => (showContext ? contextAround(view, q) : []), [showContext, view, q]);
  return (
    <article className="ov-block" data-current={isCurrent} ref={ref} aria-current={isCurrent ? "true" : undefined}>
      <header>
        <span className="mono">{at}</span>
        <span>{t("overlay.them")}</span>
        <span className="grow" />
        <StatusText q={q} />
      </header>
      <button className="ov-question" aria-expanded={showContext} onClick={() => setShowContext(!showContext)}>
        {q.text}
      </button>
      {showContext && (
        <div className="ov-context">
          {q.tags.length > 0 && (
            <div className="ov-tags">
              {q.tags.map((tag) => (
                <span className="ov-tag" key={tag}>
                  {tag}
                </span>
              ))}
            </div>
          )}
          {q.subQuestions.length > 1 && (
            <ol style={{ margin: "0 0 6px", paddingLeft: 18 }}>
              {q.subQuestions.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ol>
          )}
          {context.map((l, i) => (
            <ContextLine key={i} line={l} start={view.startedAt} />
          ))}
        </div>
      )}
      <div className="ov-hints-label">
        <span>{t("overlay.hints")}</span>
        <span>
          {q.status === "pending"
            ? t("overlay.queued")
            : total > 0
              ? t("overlay.covered", { done, total })
              : ""}
        </span>
      </div>
      {vis.hidden && (
        <p className="ov-muted">{display.level === "off" ? t("practice.hintsOff") : t("practice.hintsIn", { seconds: vis.waitS })}</p>
      )}
      {!vis.hidden && q.points.length === 0 && !q.failedCode && <p className="ov-muted">{t("overlay.generating")}</p>}
      {q.failedCode && (
        <p className="ov-fail">
          {t("overlay.failed")} · <span className="mono">{q.failedCode}</span>
        </p>
      )}
      <ol className="ov-points" hidden={vis.hidden}>
        {(vis.hidden ? [] : q.points).map((p) => (
          <li className="ov-point" key={p.n} data-status={p.status}>
            <span className="num">
              {p.status === "done" ? (
                <span className="ov-check" aria-label={t("overlay.doneLabel")}>
                  ✓
                </span>
              ) : (
                p.n
              )}
            </span>
            <span className="body">
              <span className="head">{p.head}</span> {!headsOnly && <span className="rest">{p.rest}</span>}
            </span>
            {p.status === "next" && <span className="next-tag">{t("overlay.nextTag")}</span>}
          </li>
        ))}
      </ol>
      {q.answer && !vis.hidden && !headsOnly && (
        <button className="ov-link" aria-expanded={showAnswer} onClick={() => setShowAnswer(!showAnswer)}>
          {showAnswer ? t("overlay.hideAnswer") : t("overlay.fullAnswer")}
        </button>
      )}
      {showAnswer && (
        <p className="ov-answer">
          {boldSegments(q.answer).map((s, i) => (s.bold ? <strong key={i}>{s.text}</strong> : <span key={i}>{s.text}</span>))}
        </p>
      )}
    </article>
  );
}

function ContextLine({ line, start }: { line: TranscriptLine; start: number | null }) {
  const { t } = useTranslation();
  return (
    <p>
      <span className="who">
        {line.channel === "me" ? t("overlay.me") : t("overlay.them")}
        {start !== null && <span className="mono"> {clock(line.t0Ms - start)}</span>}
      </span>
      {line.text}
    </p>
  );
}

function ErrorToast({ errors, docsUrl }: { errors: SessionErrorView[]; docsUrl: string }) {
  const { t } = useTranslation();
  const [dismissed, setDismissed] = useState(0);
  const last = errors[errors.length - 1];
  if (!last || last.at <= dismissed) return null;
  return (
    <div className="ov-toast" role="alert">
      <strong>{errorTitle(last.code)}</strong>
      <p>{errorFix(last.code)}</p>
      {last.detail && <p className="ov-detail mono">{last.detail}</p>}
      <div className="row">
        <span className="code mono">{last.code}</span>
        <button className="ov-link" onClick={() => void backend().invoke("open_url", { url: errorDocsUrl(docsUrl, last.code) })}>
          {t("errors.seeFix")}
        </button>
        <button className="ov-link" onClick={() => void backend().invoke("open_window", { view: "settings", section: "log" })}>
          {t("errors.openLog")}
        </button>
        <button className="ov-link" onClick={() => setDismissed(last.at)}>
          {t("common.dismiss")}
        </button>
      </div>
    </div>
  );
}

/** Hints were slow several times in a row: say which stage and what to change. */
function SlowToast({ view, settings }: { view: SessionView; settings: Settings | null }) {
  const { t } = useTranslation();
  const tip = view.lastLatency && settings ? latencyTip(view.lastLatency, settings) : null;
  const open = (section: string) => void backend().invoke("open_window", { view: "settings", section });
  return (
    <div className="ov-toast" role="status" style={{ borderColor: "rgba(245,184,65,.5)", background: "rgba(245,184,65,.08)" }}>
      <strong>{t("overlay.slowHints", { value: seconds(view.latencyWarning ?? 0) })}</strong>
      {view.lastLatency && <p>{breakdownText(t, view.lastLatency)}</p>}
      <p>{tip ? t(`latencyTips.${tip.key}`, { value: seconds(settings?.audio.endpointSilenceMs ?? 700) }) : t("overlay.probeAgain")}</p>
      <div className="row">
        <span className="code" />
        {tip && tip.section !== "probe" && (
          <button className="ov-link" onClick={() => open(tip.section)}>
            {t("overlay.openFix")}
          </button>
        )}
        <button className="ov-link" onClick={() => open("probe")}>
          {t("overlay.openProbe")}
        </button>
      </div>
    </div>
  );
}

function breakdownText(t: (key: string, values?: Record<string, string>) => string, b: NonNullable<SessionView["lastLatency"]>): string {
  return t("overlay.latencyBreakdown", {
    total: seconds(b.totalMs),
    asr: seconds(b.asrMs),
    detect: seconds(b.detectMs),
    generate: seconds(b.generateMs),
  });
}

function Footer({ view, settings }: { view: SessionView; settings: Settings | null }) {
  const { t } = useTranslation();
  const last = view.latencies.length ? view.latencies[view.latencies.length - 1] : null;
  const median = medianLatency(view);
  const hk = settings?.hotkeys;
  const keys = footerKeys(hk?.toggleCapture, hk?.focusPrev, hk?.focusNext, hk?.markAnswered);
  return (
    <footer className="ov-footer">
      <span className="keys">
        {keys.prefix && <kbd>{keys.prefix}</kbd>}
        <span>
          <kbd>{keys.toggle}</kbd> {t("overlay.keyToggle")}
        </span>
        <span>
          <kbd>{keys.focus}</kbd> {t("overlay.keyFocus")}
        </span>
        <span>
          <kbd>{keys.answered}</kbd> {t("overlay.keyAnswered")}
        </span>
      </span>
      <button
        className="ov-icon latency mono"
        data-slow={median !== null && median > SLOW_MS}
        title={
          median !== null
            ? [view.lastLatency && breakdownText(t, view.lastLatency), t("overlay.latencyMedian", { value: seconds(median) })].filter(Boolean).join("\n")
            : undefined
        }
        onClick={() => void backend().invoke("open_window", { view: "settings", section: "probe" })}
      >
        {last !== null ? t("overlay.latency", { value: seconds(last) }) : t("overlay.latencyNone")}
      </button>
    </footer>
  );
}

/** Shows a shared modifier prefix (e.g. `Ctrl+Alt+`) once to keep the footer on one line. */
export function footerKeys(toggle = "Ctrl+Alt+Space", prev = "Ctrl+Alt+[", next = "Ctrl+Alt+]", answered = "Ctrl+Alt+Enter") {
  const prefixOf = (k: string) => k.slice(0, k.lastIndexOf("+") + 1);
  const p = prefixOf(toggle);
  const shared = p && [prev, next, answered].every((k) => prefixOf(k) === p);
  const strip = (k: string) => (shared ? k.slice(p.length) : k);
  return {
    prefix: shared ? p : "",
    toggle: strip(toggle),
    focus: `${strip(prev)} ${strip(next)}`,
    answered: strip(answered),
  };
}

export function MiniBar({ view, display = FULL, onExpand }: { view: SessionView; display?: HintDisplay; onExpand: () => void }) {
  const { t } = useTranslation();
  const { current, next } = neighbours(view);
  // The mini bar has no per-question timer, so "delayed" behaves like "off" here.
  const hidden = display.level === "off" || display.level === "delayed";
  const point = current && !hidden ? nextPoint(current) : null;
  const cov = current ? coverageCount(current) : null;
  const pending = view.order.filter((id) => view.questions[id]?.status === "pending").length;
  return (
    <div className="ov ov-mini" role="application" aria-label={t("overlay.title")}>
      <div className="top" data-tauri-drag-region>
        <span className="ov-dot" data-on={view.state === "running"} aria-hidden />
        <span className="short" data-tauri-drag-region>
          {current?.short ?? t("overlay.emptyTitle")}
        </span>
        {cov && cov.total > 0 && <span className="ov-muted">{t("overlay.covered", { done: cov.done, total: cov.total })}</span>}
        <button className="ov-icon" onClick={onExpand} title={t("overlay.toFull")}>
          {t("overlay.expand")}
        </button>
      </div>
      <div className="big" aria-live="polite">
        {point ? (
          <>
            {point.head}
            {display.level !== "headsOnly" && <small>{point.rest}</small>}
          </>
        ) : current && hidden ? (
          <small>{t("practice.hintsOff")}</small>
        ) : current && current.points.length > 0 ? (
          <small>{t("overlay.allCovered")}</small>
        ) : (
          <small>{current ? t("overlay.generating") : t("overlay.emptyBody")}</small>
        )}
      </div>
      <div className="after">
        <span>
          {t("overlay.next")}: {next?.short ?? "—"}
        </span>
        {pending > 0 && <span>· {t("overlay.pendingCount", { count: pending })}</span>}
      </div>
    </div>
  );
}
