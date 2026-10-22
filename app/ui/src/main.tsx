import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { useTranslation } from "react-i18next";
import "./design/tokens.css";
import "./design/app.css";
import "./i18n";
import { backend } from "./backend";
import type { View } from "./backend/types";
import { Overlay } from "./overlay/Overlay";
import { Practice } from "./practice/Practice";
import { Review } from "./practice/Review";
import { SettingsWindow } from "./settings/Settings";
import { useSettings } from "./state/hooks";
import { Wizard } from "./wizard/Wizard";

const VIEWS: View[] = ["overlay", "settings", "wizard", "practice", "review"];

function currentView(): View | null {
  const v = new URLSearchParams(window.location.search).get("view");
  return VIEWS.includes(v as View) ? (v as View) : null;
}

/** Browser preview: the overlay next to links to every window. */
function Gallery() {
  const { t } = useTranslation();
  const [settings, save] = useSettings();
  useEffect(() => {
    document.documentElement.classList.add("overlay-view");
  }, []);
  return (
    <div className="gallery">
      <div className="frame">
        <Overlay />
      </div>
      <aside>
        <h1 style={{ marginTop: 0 }}>{t("gallery.title")}</h1>
        <p>{t("gallery.lead")}</p>
        <p>
          <button className="btn btn-primary" onClick={() => void backend().invoke("toggle_capture")}>
            {t("gallery.start")}
          </button>{" "}
          <button className="btn" onClick={() => void backend().invoke("user_action", { action: { type: "markAnswered" } })}>
            {t("shortcuts.markAnswered")}
          </button>
        </p>
        <ul>
          {VIEWS.filter((v) => v !== "overlay").map((v) => (
            <li key={v}>
              <a href={`?view=${v}`} target="_blank" rel="noreferrer">
                {t(`gallery.view.${v}`)}
              </a>
            </li>
          ))}
        </ul>
        {settings && (
          <p>
            <label>
              {t("settings.uiLanguage")}{" "}
              <select
                value={settings.general.uiLanguage}
                onChange={(e) =>
                  void save({ ...settings, general: { ...settings.general, uiLanguage: e.target.value as typeof settings.general.uiLanguage } })
                }
              >
                <option value="system">{t("settings.followSystem")}</option>
                <option value="en">English</option>
                <option value="zh-CN">简体中文</option>
              </select>
            </label>
          </p>
        )}
      </aside>
    </div>
  );
}

function App() {
  const view = currentView();
  const [title] = useState(() => document.title);
  const { t } = useTranslation();
  useEffect(() => {
    const cls = view === "overlay" ? "overlay-view" : "app-view";
    document.documentElement.classList.add(cls);
    document.title = view ? `${t(`gallery.view.${view}`)} · MeetCue` : title;
  }, [view, t, title]);
  switch (view) {
    case "overlay":
      return <Overlay />;
    case "settings":
      return <SettingsWindow />;
    case "wizard":
      return <Wizard />;
    case "practice":
      return <Practice />;
    case "review":
      return <Review />;
    default:
      return <Gallery />;
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
