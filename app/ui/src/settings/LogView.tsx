import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { LogLine } from "../bindings/LogLine";

const KEEP = 1000;

export interface ParsedLine {
  time: string;
  level: string;
  message: string;
}

/** Splits a log line ("2026-10-24T22:10:00.12Z  INFO meetcue::session: started") into its parts. */
export function parseLine(line: string): ParsedLine {
  const m = /^(\S+)\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+[\w:]+:\s?(.*)$/.exec(line);
  if (!m) return { time: "", level: "", message: line };
  const d = new Date(m[1]);
  const time = Number.isNaN(d.getTime()) ? "" : d.toTimeString().slice(0, 8);
  return { time, level: m[2], message: m[3] };
}

/** The application log as it is written, newest at the bottom. */
export function LogView() {
  const { t } = useTranslation();
  const [lines, setLines] = useState<LogLine[]>([]);
  const [problemsOnly, setProblemsOnly] = useState(false);
  const [follow, setFollow] = useState(true);
  const [copied, setCopied] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const last = useRef(0);

  useEffect(() => {
    let alive = true;
    const poll = () =>
      backend()
        .invoke("recent_logs", { after: last.current })
        .then((more) => {
          if (!alive || !more.length) return;
          last.current = more[more.length - 1].seq;
          setLines((l) => [...l, ...more].slice(-KEEP));
        })
        .catch(() => undefined);
    void poll();
    const id = window.setInterval(() => void poll(), 1000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  const shown = problemsOnly ? lines.filter((l) => /\s(WARN|ERROR)\s/.test(l.line)) : lines;

  // Stay at the newest line unless the user scrolled up to read.
  useEffect(() => {
    if (follow && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [shown.length, follow]);

  const copy = () =>
    void navigator.clipboard
      ?.writeText(shown.map((l) => l.line).join("\n"))
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      });

  return (
    <div id="log">
      <div className="row" style={{ marginBottom: 6 }}>
        <label className="row">
          <input type="checkbox" checked={problemsOnly} onChange={(e) => setProblemsOnly(e.target.checked)} />
          {t("diag.logProblemsOnly")}
        </label>
        <span className="spacer" />
        {!follow && (
          <button className="btn-link" onClick={() => setFollow(true)}>
            {t("diag.logLatest")}
          </button>
        )}
        <button className="btn btn-small" onClick={copy} disabled={!shown.length}>
          {copied ? t("diag.logCopied") : t("diag.logCopy")}
        </button>
      </div>
      <div
        ref={box}
        className="log-view"
        role="log"
        aria-label={t("diag.logTitle")}
        tabIndex={0}
        onScroll={(e) => {
          const el = e.currentTarget;
          setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 24);
        }}
      >
        {shown.map((l) => {
          const p = parseLine(l.line);
          return (
            <div key={l.seq} className="log-line" data-level={p.level}>
              <span className="log-time">{p.time}</span>
              <span className="log-level">{p.level}</span>
              <span className="log-msg">{p.message}</span>
            </div>
          );
        })}
        {!shown.length && <p className="muted">{problemsOnly ? t("diag.logNoProblems") : t("diag.logEmpty")}</p>}
      </div>
    </div>
  );
}
