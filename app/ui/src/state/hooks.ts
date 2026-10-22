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
}

type Action = { kind: "event"; e: MeetCueEvent } | { kind: "replay"; events: MeetCueEvent[] };

function sessionReducer(s: SessionView, a: Action): SessionView {
  return a.kind === "event" ? reduce(s, a.e) : a.events.reduce(reduce, emptySession());
}

/** Subscribes to pipeline events, replaying what happened before the window opened. */
export function useSession(): SessionHandle {
  const [view, dispatch] = useReducer(sessionReducer, undefined, emptySession);
  const [wallStart, setWallStart] = useState<number | null>(null);
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
          if (started.current) setWallStart(Date.now() - last.tMs);
        }
        replayed = true;
        for (const e of queued) dispatch({ kind: "event", e });
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return { view, wallStart };
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
