import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { backend } from "../backend";
import type { AppEvent } from "../bindings/AppEvent";
import type { AppInfo } from "../bindings/AppInfo";
import type { Settings } from "../bindings/Settings";
import { resolveLanguage, setLanguage } from "../i18n";
import { emptySession, reduce, type SessionView } from "./session";
import type { MeetCueEvent } from "../bindings/MeetCueEvent";

export function useAppInfo(): AppInfo | null {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    let alive = true;
    void backend()
      .invoke("get_app_info")
      .then((i) => alive && setInfo(i));
    return () => {
      alive = false;
    };
  }, []);
  return info;
}

/** Applies the language and accent color from settings to the document. */
export function applyAppearance(settings: Settings, systemLocale: string): void {
  setLanguage(resolveLanguage(settings.general.uiLanguage === "system" ? null : settings.general.uiLanguage, systemLocale));
  document.documentElement.style.setProperty("--accent", settings.general.accent || "#F5B841");
}

/** Current settings, kept in sync across windows. */
export function useSettings(): [Settings | null, (s: Settings) => Promise<Settings>] {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    const b = backend();
    void Promise.all([b.invoke("get_settings"), b.invoke("get_app_info")]).then(([s, info]) => {
      if (!alive) return;
      setSettings(s);
      applyAppearance(s, info.systemLocale);
      void b
        .onAppEvent((e: AppEvent) => {
          if (e.type === "settingsChanged") {
            setSettings(e.settings);
            applyAppearance(e.settings, info.systemLocale);
          }
        })
        .then((u) => {
          if (alive) unlisten = u;
          else u();
        });
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  const save = useCallback(async (s: Settings) => {
    const saved = await backend().invoke("save_settings", { settings: s });
    setSettings(saved);
    return saved;
  }, []);
  return [settings, save];
}

export interface SessionHandle {
  view: SessionView;
  /** Wall-clock time (Date.now()) when the session started. */
  wallStart: number | null;
  /** Time spent paused, so the meeting clock stops while paused. */
  pauses: PauseClock;
}

export interface PauseClock {
  /** Total of finished pauses, in milliseconds. */
  pausedMs: number;
  /** Wall-clock start of the current pause, if paused now. */
  pausedAt: number | null;
}

export const NO_PAUSES: PauseClock = { pausedMs: 0, pausedAt: null };

/** Updates the pause clock for a session state change at wall time `at`. */
export function trackPause(c: PauseClock, state: string, at: number): PauseClock {
  if (state === "paused") return c.pausedAt === null ? { ...c, pausedAt: at } : c;
  if (state === "running") return c.pausedAt === null ? c : { pausedMs: c.pausedMs + Math.max(0, at - c.pausedAt), pausedAt: null };
  if (state === "stopped" || state === "idle") return NO_PAUSES;
  return c;
}

/** Listening time: time since the start minus the time spent paused. */
export function activeMs(now: number, wallStart: number, c: PauseClock): number {
  return Math.max(0, (c.pausedAt ?? now) - wallStart - c.pausedMs);
}

type Action = { kind: "event"; e: MeetCueEvent } | { kind: "replay"; events: MeetCueEvent[] };

function sessionReducer(s: SessionView, a: Action): SessionView {
  return a.kind === "event" ? reduce(s, a.e) : a.events.reduce(reduce, emptySession());
}

/** Subscribes to pipeline events, replaying what happened before the window opened. */
export function useSession(): SessionHandle {
  const [view, dispatch] = useReducer(sessionReducer, undefined, emptySession);
  const [wallStart, setWallStart] = useState<number | null>(null);
  const [pauses, setPauses] = useState<PauseClock>(NO_PAUSES);
  const started = useRef(false);
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    const b = backend();
    const queued: MeetCueEvent[] = [];
    let replayed = false;
    void b
      .onEvent((e) => {
        if (!alive) return;
        if (e.type === "sessionState" && e.state === "running" && !started.current) {
          started.current = true;
          setWallStart(Date.now());
          setPauses(NO_PAUSES);
        } else if (e.type === "sessionState") {
          const at = Date.now();
          setPauses((c) => trackPause(c, e.state, at));
        }
        if (e.type === "sessionState" && e.state === "stopped") started.current = false;
        if (replayed) dispatch({ kind: "event", e });
        else queued.push(e);
      })
      .then(async (u) => {
        unlisten = u;
        const history = await b.invoke("session_history");
        if (!alive) return;
        dispatch({ kind: "replay", events: history.map((t) => t.event) });
        if (history.length) {
          const last = history[history.length - 1];
          started.current = history.some((t) => t.event.type === "sessionState" && t.event.state === "running");
          if (started.current) {
            const start = Date.now() - last.tMs;
            setWallStart(start);
            let c = NO_PAUSES;
            for (const t of history) {
              if (t.event.type === "sessionState" && t.event.state !== "running") c = trackPause(c, t.event.state, start + t.tMs);
              else if (t.event.type === "sessionState") c = trackPause(c, "running", start + t.tMs);
            }
            setPauses(c);
          }
        }
        replayed = true;
        for (const e of queued) dispatch({ kind: "event", e });
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return { view, wallStart, pauses };
}

/** Re-renders every `ms` milliseconds. */
export function useNow(ms = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(id);
  }, [ms]);
  return now;
}

export function useAppEvents(cb: (e: AppEvent) => void): void {
  const ref = useRef(cb);
  useEffect(() => {
    ref.current = cb;
  });
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    void backend()
      .onAppEvent((e) => ref.current(e))
      .then((u) => {
        if (alive) unlisten = u;
        else u();
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
}
