import { useCallback, useEffect, useState, type CSSProperties, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { PanelChange } from "../bindings/PanelChange";
import type { PanelPlace } from "../bindings/PanelPlace";
import type { Settings } from "../bindings/Settings";
import { useAppEvents } from "../state/hooks";

const change = (c: PanelChange) => void backend().invoke("change_panel", { change: c });

const INTERACTIVE = "button, a, input, select, textarea, label, [data-no-drag]";

/**
 * Starts moving the panel from a press anywhere on it that is not a control
 * or a scroll bar, so there is always a large area to grab. Unlike the
 * shell's drag regions this never maximizes the panel on a double click, so
 * a touchpad's double-tap-and-drag moves it.
 */
export function dragHandle(e: MouseEvent) {
  if (e.button !== 0) return;
  const target = e.target as HTMLElement;
  if (target.closest(INTERACTIVE)) return;
  const onScrollBar =
    target.scrollHeight > target.clientHeight && e.nativeEvent.offsetX >= target.clientWidth;
  if (onScrollBar) return;
  e.preventDefault();
  void backend().startDragging();
}

/** Panel opacity and text size as CSS; the panel fades further while clicks pass through it. */
export function panelStyle(settings: Settings | null): CSSProperties {
  const o = settings?.overlay;
  const opacity = o ? (o.clickThrough ? Math.min(o.opacity, 0.6) : o.opacity) : 1;
  return { opacity, ["--ov-scale" as string]: o?.textScale ?? 1 };
}

/** A short line after a spot shortcut: "Moved to spot 1" or "Saved as spot 1". */
export function useSpotFlash(): string | null {
  const { t } = useTranslation();
  const [flash, setFlash] = useState<{ text: string; at: number } | null>(null);
  useAppEvents((e) => {
    if (e.type === "panelSpot") {
      const n = e.slot + 1;
      setFlash({ text: e.saved ? t("placement.savedSpot", { n }) : t("placement.wentToSpot", { n }), at: Date.now() });
    }
  });
  useEffect(() => {
    if (!flash) return;
    const id = window.setTimeout(() => setFlash(null), 2500);
    return () => window.clearTimeout(id);
  }, [flash]);
  return flash?.text ?? null;
}

/** Shown while clicks pass through the panel, with the way back. */
export function ClickThroughBar({ settings }: { settings: Settings | null }) {
  const { t } = useTranslation();
  if (!settings?.overlay.clickThrough) return null;
  return (
    <div className="ov-through" role="status">
      {t("placement.throughOn", { key: settings.hotkeys.clickThrough })}
    </div>
  );
}

const GRID: (PanelPlace | "nextScreen" | null)[] = [
  "topLeft",
  "top",
  "topRight",
  "left",
  "nextScreen",
  "right",
  "bottomLeft",
  "bottom",
  "bottomRight",
];

const ARROW: Record<PanelPlace, string> = {
  topLeft: "↖",
  top: "↑",
  topRight: "↗",
  left: "←",
  right: "→",
  bottomLeft: "↙",
  bottom: "↓",
  bottomRight: "↘",
};

/**
 * Everything for putting the panel where it doesn't cover the meeting, in
 * large one-click targets: snap to an edge or corner, saved spots, size,
 * see-through and hiding it for a moment.
 */
export function PlacementMenu({ settings, onClose }: { settings: Settings; onClose: () => void }) {
  const { t } = useTranslation();
  const [screens, setScreens] = useState(1);
  useEffect(() => {
    void backend()
      .invoke("panel_screens")
      .then((n) => setScreens(n));
  }, []);
  const o = settings.overlay;
  const hk = settings.hotkeys;
  const spotKeys = [hk.spot1, hk.spot2, hk.spot3];
  const closeAfter = useCallback(
    (c: PanelChange) => {
      change(c);
      onClose();
    },
    [onClose],
  );
  const pct = Math.round(o.opacity * 100);
  const textPct = Math.round(o.textScale * 100);
  const step = (v: number, d: number, lo: number, hi: number) => Math.round(Math.min(hi, Math.max(lo, v + d)) * 100) / 100;

  return (
    <div className="ov-place" role="dialog" aria-label={t("placement.title")} data-no-drag>
      <div className="ov-place-head">
        <strong>{t("placement.title")}</strong>
        <span className="grow" />
        <button className="ov-icon" onClick={onClose}>
          {t("placement.done")}
        </button>
      </div>

      <section>
        <h2>{t("placement.snap")}</h2>
        <div className="ov-snap" role="group" aria-label={t("placement.snap")}>
          {GRID.map((p, i) =>
            p === "nextScreen" ? (
              screens > 1 ? (
                <button key={i} className="ov-snap-cell" onClick={() => change({ type: "nextScreen" })} title={t("placement.nextScreen")}>
                  ⇄<span className="sr">{t("placement.nextScreen")}</span>
                </button>
              ) : (
                <span key={i} className="ov-snap-cell ov-snap-me" aria-hidden />
              )
            ) : p ? (
              <button
                key={i}
                className="ov-snap-cell"
                aria-label={t(`placement.place.${p}`)}
                title={t(`placement.place.${p}`)}
                onClick={() => change({ type: "snap", place: p })}
              >
                {ARROW[p]}
              </button>
            ) : null,
          )}
        </div>
      </section>

      <section>
        <h2>{t("placement.spots")}</h2>
        <p className="ov-muted ov-place-lead">{t("placement.spotsLead")}</p>
        {o.spots.map((spot, slot) => (
          <div className="ov-spot" key={slot}>
            <span className="ov-spot-n">{slot + 1}</span>
            <kbd>{spotKeys[slot]}</kbd>
            <span className="grow ov-muted">{spot ? t(`settings.panel${spot.mode === "mini" ? "Mini" : "Full"}`) : t("placement.empty")}</span>
            {spot && (
              <button className="ov-chip" onClick={() => change({ type: "goSpot", slot })}>
                {t("placement.go")}
              </button>
            )}
            <button className="ov-chip" onClick={() => change({ type: "saveSpot", slot })}>
              {spot ? t("placement.replace") : t("placement.saveHere")}
            </button>
            {spot && (
              <button className="ov-chip" aria-label={t("placement.clear", { n: slot + 1 })} onClick={() => change({ type: "clearSpot", slot })}>
                ×
              </button>
            )}
          </div>
        ))}
      </section>

      <section>
        <h2>{t("placement.size")}</h2>
        <div className="ov-chips" role="group" aria-label={t("placement.width")}>
          <span className="ov-muted">{t("placement.width")}</span>
          {(["narrow", "normal", "wide"] as const).map((w) => (
            <button key={w} className="ov-chip" onClick={() => change({ type: "width", width: w })}>
              {t(`placement.widths.${w}`)}
            </button>
          ))}
        </div>
        <div className="ov-chips" role="group" aria-label={t("placement.height")}>
          <span className="ov-muted">{t("placement.height")}</span>
          {(["third", "half", "tall"] as const).map((h) => (
            <button key={h} className="ov-chip" onClick={() => change({ type: "height", height: h })}>
              {t(`placement.heights.${h}`)}
            </button>
          ))}
          <button className="ov-chip" onClick={() => void backend().invoke("set_panel_mode", { mode: o.mode === "mini" ? "full" : "mini" })}>
            {o.mode === "mini" ? t("settings.panelFull") : t("settings.panelMini")}
          </button>
        </div>
      </section>

      <section>
        <h2>{t("placement.seeThrough")}</h2>
        <div className="ov-chips">
          <span className="ov-muted">{t("placement.opacity")}</span>
          <button className="ov-chip" aria-label={t("placement.lessOpaque")} onClick={() => change({ type: "opacity", value: step(o.opacity, -0.1, 0.3, 1) })}>
            −
          </button>
          <span className="mono ov-val">{pct}%</span>
          <button className="ov-chip" aria-label={t("placement.moreOpaque")} onClick={() => change({ type: "opacity", value: step(o.opacity, 0.1, 0.3, 1) })}>
            +
          </button>
        </div>
        <div className="ov-chips">
          <span className="ov-muted">{t("placement.textSize")}</span>
          <button className="ov-chip" aria-label={t("placement.smallerText")} onClick={() => change({ type: "textScale", value: step(o.textScale, -0.1, 0.8, 1.6) })}>
            A−
          </button>
          <span className="mono ov-val">{textPct}%</span>
          <button className="ov-chip" aria-label={t("placement.largerText")} onClick={() => change({ type: "textScale", value: step(o.textScale, 0.1, 0.8, 1.6) })}>
            A+
          </button>
        </div>
        <button className="ov-wide" onClick={() => closeAfter({ type: "peek" })}>
          <strong>{t("placement.peek")}</strong>
          <span className="ov-muted">
            {t("placement.peekHint")} <kbd>{hk.peek}</kbd>
          </span>
        </button>
        <button className="ov-wide" onClick={() => closeAfter({ type: "clickThrough", on: true })}>
          <strong>{t("placement.through")}</strong>
          <span className="ov-muted">
            {t("placement.throughHint")} <kbd>{hk.clickThrough}</kbd>
          </span>
        </button>
      </section>
    </div>
  );
}
