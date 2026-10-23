import * as Tabs from "@radix-ui/react-tabs";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { DeviceList } from "../bindings/DeviceList";
import type { HotkeyState } from "../bindings/HotkeyState";
import type { Hotkeys } from "../bindings/Hotkeys";
import type { MeetCueEvent } from "../bindings/MeetCueEvent";
import type { PriceTable } from "../bindings/PriceTable";
import type { ProbeReport } from "../bindings/ProbeReport";
import type { Settings } from "../bindings/Settings";
import type { UsageSummary } from "../bindings/UsageSummary";
import { Badge, Field, Section, Switch } from "../design/components";
import { useAppEvents, useAppInfo } from "../state/hooks";
import { millis, usd } from "../util/format";
import { errorTitle } from "../util/errors";
import { KeyField } from "./Keys";
import { ModelPicker } from "./ModelPicker";
import { ProbePanel } from "./Probe";
import { PROVIDERS, providerName } from "./providers";
import { useDraft } from "./useDraft";

type Update = (f: (s: Settings) => Settings) => Promise<void>;

const TABS = ["general", "audio", "providers", "hints", "shortcuts", "diagnostics"] as const;
type Tab = (typeof TABS)[number];

function initialTab(): { tab: Tab; anchor: string | null } {
  const hash = window.location.hash.replace("#", "");
  if ((TABS as readonly string[]).includes(hash)) return { tab: hash as Tab, anchor: null };
  if (hash === "probe" || hash === "keys" || hash === "budget") return { tab: "providers", anchor: hash };
  return { tab: "general", anchor: null };
}

export function SettingsWindow() {
  const { t } = useTranslation();
  const { settings, update } = useDraft();
  const [{ tab, anchor }, setNav] = useState(initialTab);
  useEffect(() => {
    if (anchor) window.setTimeout(() => document.getElementById(anchor)?.scrollIntoView({ block: "start" }), 50);
  }, [anchor, tab]);
  if (!settings) return <p className="app-body muted">{t("common.loading")}</p>;
  return (
    <Tabs.Root className="tabs" value={tab} onValueChange={(v) => setNav({ tab: v as Tab, anchor: null })} orientation="vertical">
      <Tabs.List className="tabs-list" aria-label={t("settings.title")}>
        {TABS.map((id) => (
          <Tabs.Trigger key={id} value={id} className="tabs-trigger">
            {t(`settings.tab.${id}`)}
          </Tabs.Trigger>
        ))}
      </Tabs.List>
      <Tabs.Content value="general" className="tabs-content">
        <GeneralTab settings={settings} update={update} />
      </Tabs.Content>
      <Tabs.Content value="audio" className="tabs-content">
        <AudioTab settings={settings} update={update} />
      </Tabs.Content>
      <Tabs.Content value="providers" className="tabs-content">
        <ProvidersTab settings={settings} update={update} />
      </Tabs.Content>
      <Tabs.Content value="hints" className="tabs-content">
        <HintsTab settings={settings} update={update} />
      </Tabs.Content>
      <Tabs.Content value="shortcuts" className="tabs-content">
        <ShortcutsTab settings={settings} update={update} />
      </Tabs.Content>
      <Tabs.Content value="diagnostics" className="tabs-content">
        <DiagnosticsTab settings={settings} update={update} />
      </Tabs.Content>
    </Tabs.Root>
  );
}

export function LanguageField({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  return (
    <Field label={t("settings.uiLanguage")} htmlFor="ui-lang" hint={t("settings.uiLanguageHint")}>
      <select
        id="ui-lang"
        value={settings.general.uiLanguage}
        onChange={(e) =>
          void update((s) => {
            s.general.uiLanguage = e.target.value as Settings["general"]["uiLanguage"];
            return s;
          })
        }
      >
        <option value="system">{t("settings.followSystem")}</option>
        <option value="en">English</option>
        <option value="zh-CN">简体中文</option>
      </select>
    </Field>
  );
}

function GeneralTab({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  return (
    <>
      <h2 className="page">{t("settings.tab.general")}</h2>
      <Section>
        <LanguageField settings={settings} update={update} />
        <Field label={t("settings.hintLanguage")} htmlFor="hint-lang" hint={t("settings.hintLanguageHint")}>
          <select
            id="hint-lang"
            value={settings.general.hintLanguage}
            onChange={(e) =>
              void update((s) => {
                s.general.hintLanguage = e.target.value as Settings["general"]["hintLanguage"];
                return s;
              })
            }
          >
            <option value="followQuestion">{t("settings.followQuestion")}</option>
            <option value="zh-CN">简体中文</option>
            <option value="en">English</option>
          </select>
        </Field>
        <Field label={t("settings.accent")} htmlFor="accent">
          <input
            id="accent"
            type="color"
            value={settings.general.accent}
            onChange={(e) =>
              void update((s) => {
                s.general.accent = e.target.value;
                return s;
              })
            }
          />
          <button
            className="btn btn-small"
            onClick={() =>
              void update((s) => {
                s.general.accent = "#F5B841";
                return s;
              })
            }
          >
            {t("common.reset")}
          </button>
        </Field>
        <Field label={t("settings.startup")}>
          <Switch
            checked={settings.general.autostart}
            label={t("settings.autostart")}
            onChange={(v) =>
              void update((s) => {
                s.general.autostart = v;
                return s;
              })
            }
          />
        </Field>
      </Section>
      <Section title={t("settings.overlayTitle")}>
        <Switch
          checked={settings.overlay.hideInScreenShare}
          label={t("settings.hideInShare")}
          description={t("settings.hideInShareHint")}
          onChange={(v) =>
            void update((s) => {
              s.overlay.hideInScreenShare = v;
              return s;
            })
          }
        />
        <Switch
          checked={settings.overlay.clickThrough}
          label={t("settings.clickThrough")}
          description={t("settings.clickThroughHint")}
          onChange={(v) =>
            void update((s) => {
              s.overlay.clickThrough = v;
              return s;
            })
          }
        />
      </Section>
    </>
  );
}

function useLevels(): { me: number; remote: number } {
  const [levels, setLevels] = useState({ me: -100, remote: -100 });
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    void backend()
      .onEvent((e: MeetCueEvent) => {
        if (e.type === "levels") setLevels({ me: e.me, remote: e.remote });
      })
      .then((u) => {
        if (alive) unlisten = u;
        else u();
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return levels;
}

function LevelBar({ db, label }: { db: number; label: string }) {
  const pct = Math.max(0, Math.min(100, ((db + 70) / 70) * 100));
  return (
    <span className="row">
      <span className="meter-bar" role="meter" aria-label={label} aria-valuemin={-70} aria-valuemax={0} aria-valuenow={Math.round(db)}>
        <span style={{ width: `${pct}%` }} />
      </span>
      <span className="mono muted">{db <= -99 ? "—" : `${Math.round(db)} dB`}</span>
    </span>
  );
}

export function AudioFields({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const [devices, setDevices] = useState<DeviceList | null>(null);
  const [preview, setPreview] = useState(false);
  const levels = useLevels();
  const info = useAppInfo();
  useEffect(() => {
    void backend()
      .invoke("list_devices")
      .then(setDevices)
      .catch(() => setDevices({ microphones: [], outputs: [] }));
  }, []);
  useEffect(() => {
    void backend().invoke("audio_preview", { on: preview });
  }, [preview]);
  useEffect(() => () => void backend().invoke("audio_preview", { on: false }), []);
  const mic = devices?.microphones.find((d) => (settings.audio.micDevice ? d.id === settings.audio.micDevice : d.isDefault));
  return (
    <>
      {info && !info.captureSupported && <p className="note note-warn">{errorTitle("MC-AUD-900")}</p>}
      <Field label={t("audio.microphone")} htmlFor="mic">
        <select
          id="mic"
          className="input-wide"
          value={settings.audio.micDevice ?? ""}
          onChange={(e) =>
            void update((s) => {
              s.audio.micDevice = e.target.value || null;
              return s;
            })
          }
        >
          <option value="">{t("audio.systemDefault")}</option>
          {devices?.microphones.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
              {d.isDefault ? ` (${t("common.default")})` : ""}
            </option>
          ))}
        </select>
      </Field>
      {mic?.isBluetooth && <p className="note note-warn">{t("audio.bluetoothWarning")}</p>}
      <Field label={t("audio.remote")} htmlFor="remote" hint={t("audio.remoteHint")}>
        <select id="remote" value={settings.audio.remoteSource} onChange={() => undefined}>
          <option value="allSystemSound">{t("audio.allSystemSound")}</option>
          <option value="meetingAppOnly" disabled>
            {t("audio.meetingAppOnly")}
          </option>
        </select>
      </Field>
      <Field label={t("audio.languages")} htmlFor="speech-lang" hint={t("audio.languagesHint")}>
        <select
          id="speech-lang"
          value={settings.audio.languages.join(",")}
          onChange={(e) =>
            void update((s) => {
              s.audio.languages = e.target.value.split(",");
              return s;
            })
          }
        >
          <option value="zh,en">{t("audio.langZhEn")}</option>
          <option value="en">{t("audio.langEn")}</option>
          <option value="zh">{t("audio.langZh")}</option>
        </select>
      </Field>
      <Field label={t("audio.levels")} hint={t("audio.levelsHint")}>
        <div style={{ display: "grid", gap: 6 }}>
          <span className="row">
            <span style={{ width: 120 }}>{t("overlay.sourceMe")}</span>
            <LevelBar db={levels.me} label={t("overlay.sourceMe")} />
          </span>
          <span className="row">
            <span style={{ width: 120 }}>{t("overlay.sourceRemote")}</span>
            <LevelBar db={levels.remote} label={t("overlay.sourceRemote")} />
          </span>
          <span>
            <button className="btn btn-small" aria-pressed={preview} onClick={() => setPreview(!preview)}>
              {preview ? t("audio.stopPreview") : t("audio.startPreview")}
            </button>
          </span>
        </div>
      </Field>
    </>
  );
}

function AudioTab({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  return (
    <>
      <h2 className="page">{t("settings.tab.audio")}</h2>
      <Section>
        <AudioFields settings={settings} update={update} />
      </Section>
      <Section title={t("audio.advanced")}>
        <Field label={t("audio.vadThreshold")} htmlFor="vad" hint={t("audio.vadHint")}>
          <input
            id="vad"
            type="number"
            min={-70}
            max={-10}
            step={1}
            value={settings.audio.vadThresholdDb}
            onChange={(e) =>
              void update((s) => {
                s.audio.vadThresholdDb = Number(e.target.value);
                return s;
              })
            }
          />
          <span className="muted">dBFS</span>
        </Field>
        <Field label={t("audio.endpointSilence")} htmlFor="endpoint" hint={t("audio.endpointHint")}>
          <input
            id="endpoint"
            type="number"
            min={300}
            max={2000}
            step={50}
            value={settings.audio.endpointSilenceMs}
            onChange={(e) =>
              void update((s) => {
                s.audio.endpointSilenceMs = Number(e.target.value);
                return s;
              })
            }
          />
          <span className="muted">ms</span>
        </Field>
      </Section>
    </>
  );
}

export function useCredentials(): [Record<string, boolean>, () => void] {
  const [status, setStatus] = useState<Record<string, boolean>>({});
  const refresh = useCallback(() => {
    void backend().invoke("credential_status").then(setStatus);
  }, []);
  useEffect(refresh, [refresh]);
  return [status, refresh];
}

export function usePricing(): PriceTable | null {
  const [table, setTable] = useState<PriceTable | null>(null);
  useEffect(() => {
    void backend().invoke("get_pricing").then(setTable);
  }, []);
  return table;
}

function EndpointField({ settings, update, probe }: { settings: Settings; update: Update; probe: ProbeReport | null }) {
  const { t } = useTranslation();
  const provider = settings.providers.asr.provider;
  const [hosts, setHosts] = useState<string[]>([]);
  useEffect(() => {
    if (provider) void backend().invoke("provider_hosts", { provider }).then(setHosts);
  }, [provider]);
  if (!provider || hosts.length < 2) return null;
  const latency = (h: string) => probe?.results.find((r) => r.provider === provider && r.host === h)?.tls?.medianMs;
  const value = settings.providers.asr.endpoint.mode === "auto" ? "" : settings.providers.asr.endpoint.host;
  return (
    <Field label={t("providers.endpoint")} htmlFor="asr-endpoint" hint={t("providers.endpointHint")}>
      <select
        id="asr-endpoint"
        value={value}
        onChange={(e) =>
          void update((s) => {
            s.providers.asr.endpoint = e.target.value ? { mode: "host", host: e.target.value } : { mode: "auto" };
            return s;
          })
        }
      >
        <option value="">{t("providers.endpointAuto")}</option>
        {hosts.map((h) => {
          const l = latency(h);
          return (
            <option key={h} value={h}>
              {h}
              {l !== undefined ? ` · ${millis(l)} ms` : ""}
            </option>
          );
        })}
      </select>
    </Field>
  );
}

function ProvidersTab({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const [credentials, refresh] = useCredentials();
  const table = usePricing();
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  useEffect(() => {
    void backend().invoke("usage_summary", { month: null }).then(setUsage);
  }, []);
  const a = settings.providers.estimate;
  const num = (v: string) => (v.trim() === "" ? null : Number(v));
  const setAssumption = (k: keyof typeof a, v: number) =>
    update((s) => {
      (s.providers.estimate[k] as number) = v;
      return s;
    });
  return (
    <>
      <h2 className="page">{t("settings.tab.providers")}</h2>
      <Section title={t("providers.asrTitle")} lead={t("providers.asrLead")}>
        <ModelPicker slot="asr" settings={settings} update={update} table={table} probe={probe} credentials={credentials} />
        <EndpointField settings={settings} update={update} probe={probe} />
      </Section>
      <Section title={t("providers.detectTitle")} lead={t("providers.detectLead")}>
        <ModelPicker slot="detect" settings={settings} update={update} table={table} probe={probe} credentials={credentials} />
      </Section>
      <Section title={t("providers.hintsTitle")} lead={t("providers.hintsLead")}>
        <ModelPicker slot="hints" settings={settings} update={update} table={table} probe={probe} credentials={credentials} />
      </Section>
      <div id="keys" />
      <Section title={t("providers.keysTitle")} lead={t("providers.keysLead")}>
        {PROVIDERS.filter((p) => p.available).map((p) => (
          <KeyField key={p.id} provider={p} configured={!!credentials[p.id]} settings={settings} update={update} onChanged={refresh} />
        ))}
      </Section>
      <Section title={t("probe.title")}>
        <ProbePanel settings={settings} credentials={credentials} update={update} onReport={setProbe} />
      </Section>
      <Section title={t("providers.assumptionsTitle")} lead={t("providers.assumptionsLead")}>
        <Field label={t("providers.speechRatio")} htmlFor="a-speech">
          <input id="a-speech" type="number" min={0} max={1} step={0.05} value={a.speechRatio} onChange={(e) => void setAssumption("speechRatio", Number(e.target.value))} />
        </Field>
        <Field label={t("providers.questionsPerHour")} htmlFor="a-q">
          <input id="a-q" type="number" min={0} step={1} value={a.questionsPerHour} onChange={(e) => void setAssumption("questionsPerHour", Number(e.target.value))} />
        </Field>
        <Field label={t("providers.tokensPerHint")} htmlFor="a-hin">
          <input id="a-hin" type="number" min={0} step={50} value={a.hintInputTokens} onChange={(e) => void setAssumption("hintInputTokens", Number(e.target.value))} />
          <input aria-label={t("providers.outputTokens")} type="number" min={0} step={50} value={a.hintOutputTokens} onChange={(e) => void setAssumption("hintOutputTokens", Number(e.target.value))} />
        </Field>
        <Field label={t("providers.tokensPerDetect")} htmlFor="a-din">
          <input id="a-din" type="number" min={0} step={50} value={a.detectInputTokens} onChange={(e) => void setAssumption("detectInputTokens", Number(e.target.value))} />
          <input aria-label={t("providers.outputTokens")} type="number" min={0} step={10} value={a.detectOutputTokens} onChange={(e) => void setAssumption("detectOutputTokens", Number(e.target.value))} />
        </Field>
        <p className="muted" style={{ fontSize: 13 }}>
          {t("providers.estimateFormula")}
        </p>
      </Section>
      <div id="budget" />
      <Section title={t("budget.title")} lead={t("budget.lead")}>
        <Field label={t("budget.session")} htmlFor="b-session">
          <input
            id="b-session"
            type="number"
            min={0}
            step={0.5}
            value={settings.providers.budget.sessionLimitUsd ?? ""}
            onChange={(e) =>
              void update((s) => {
                s.providers.budget.sessionLimitUsd = num(e.target.value);
                return s;
              })
            }
          />
          <span className="muted">USD</span>
        </Field>
        <Field label={t("budget.monthly")} htmlFor="b-month">
          <input
            id="b-month"
            type="number"
            min={0}
            step={1}
            value={settings.providers.budget.monthlyLimitUsd ?? ""}
            onChange={(e) =>
              void update((s) => {
                s.providers.budget.monthlyLimitUsd = num(e.target.value);
                return s;
              })
            }
          />
          <span className="muted">USD</span>
        </Field>
        <Field label={t("budget.onLimit")} htmlFor="b-action">
          <select
            id="b-action"
            value={settings.providers.budget.onLimit}
            onChange={(e) =>
              void update((s) => {
                s.providers.budget.onLimit = e.target.value as Settings["providers"]["budget"]["onLimit"];
                return s;
              })
            }
          >
            <option value="notifyOnly">{t("budget.notifyOnly")}</option>
            <option value="pauseHints">{t("budget.pauseHints")}</option>
            <option value="endSession">{t("budget.endSession")}</option>
          </select>
        </Field>
      </Section>
      <Section title={t("usage.title", { month: usage?.month ?? "" })} lead={t("usage.lead")}>
        <div className="table-wrap">
          <table className="data">
            <thead>
              <tr>
                <th>{t("usage.provider")}</th>
                <th className="num">{t("usage.calls")}</th>
                <th className="num">{t("usage.tokens")}</th>
                <th className="num">{t("usage.audio")}</th>
                <th className="num">{t("usage.cost")}</th>
              </tr>
            </thead>
            <tbody>
              {usage?.rows.map((r) => (
                <tr key={`${r.provider}/${r.model}`}>
                  <td>
                    {providerName(r.provider)} <span className="muted">{r.model}</span>
                  </td>
                  <td className="num">{r.calls}</td>
                  <td className="num">{r.inputTokens + r.outputTokens ? millis(r.inputTokens + r.outputTokens) : "—"}</td>
                  <td className="num">{r.audioMs ? t("usage.minutes", { value: millis(r.audioMs / 60000) }) : "—"}</td>
                  <td className="num">{r.micros === null ? t("providers.priceUnknown") : usd(r.micros)}</td>
                </tr>
              ))}
              <tr>
                <td>
                  <strong>{t("usage.total")}</strong>
                </td>
                <td colSpan={3} />
                <td className="num">
                  <strong>{usd(usage?.totalMicros ?? 0)}</strong>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div className="row" style={{ margin: "8px 0" }}>
          <span className="muted" style={{ fontSize: 12 }}>
            {t("usage.estimateNote")}
          </span>
          <span className="spacer" />
          <button className="btn btn-small" onClick={() => void backend().invoke("export_usage_csv")}>
            {t("usage.exportCsv")}
          </button>
        </div>
      </Section>
      <Section title={t("providers.advancedTitle")}>
        <Field label={t("providers.proxy")} htmlFor="proxy" hint={t("providers.proxyHint")}>
          <input
            id="proxy"
            className="input-wide"
            type="url"
            placeholder="http://proxy:8080"
            value={settings.providers.proxy ?? ""}
            onChange={(e) =>
              void update((s) => {
                s.providers.proxy = e.target.value.trim() || null;
                return s;
              })
            }
          />
        </Field>
      </Section>
    </>
  );
}

function HintsTab({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const rule = (key: "onlyDirectedToMe" | "queueInterruptions" | "trackCoverage" | "earlyTrigger") => (
    <Switch
      checked={settings.hints[key]}
      label={t(`hints.${key}`)}
      description={t(`hints.${key}Hint`)}
      onChange={(v) =>
        void update((s) => {
          s.hints[key] = v;
          return s;
        })
      }
    />
  );
  return (
    <>
      <h2 className="page">{t("settings.tab.hints")}</h2>
      <Section>
        <Field label={t("hints.role")} htmlFor="role" hint={t("hints.roleHint")}>
          <textarea
            id="role"
            value={settings.hints.role}
            placeholder={t("hints.rolePlaceholder")}
            onChange={(e) =>
              void update((s) => {
                s.hints.role = e.target.value;
                return s;
              })
            }
          />
        </Field>
        <Field label={t("hints.materials")} hint={t("hints.materialsHint")}>
          <div style={{ display: "grid", gap: 6, width: "100%" }}>
            {settings.hints.materials.map((m, i) => (
              <div className="row" key={m.path + i}>
                <span>{m.name}</span>
                <span className="muted">{t("hints.chars", { count: m.text.length })}</span>
                <span className="spacer" />
                <button
                  className="btn btn-small"
                  onClick={() =>
                    void update((s) => {
                      s.hints.materials.splice(i, 1);
                      return s;
                    })
                  }
                >
                  {t("common.remove")}
                </button>
              </div>
            ))}
            <span>
              <button
                className="btn btn-small"
                onClick={() =>
                  void backend()
                    .invoke("import_material")
                    .then((m) => {
                      if (m)
                        void update((s) => {
                          s.hints.materials.push(m);
                          return s;
                        });
                    })
                }
              >
                {t("hints.importMaterial")}
              </button>
            </span>
          </div>
        </Field>
      </Section>
      <Section title={t("hints.rulesTitle")}>
        {rule("onlyDirectedToMe")}
        {rule("queueInterruptions")}
        {rule("trackCoverage")}
        {rule("earlyTrigger")}
      </Section>
    </>
  );
}

const HOTKEY_ORDER: (keyof Hotkeys)[] = ["toggleCapture", "cyclePanel", "focusPrev", "focusNext", "markAnswered"];

/** Builds an accelerator such as `Ctrl+Alt+K` from a key event. */
export function acceleratorFromEvent(e: Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "key" | "code">): string | null {
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
  if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return null;
  let key = e.key;
  if (e.code === "Space") key = "Space";
  else if (key === "Enter") key = "Enter";
  else if (key.length === 1) key = key.toUpperCase();
  if (!mods.length) return null;
  return [...mods, key].join("+");
}

function ShortcutsTab({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<HotkeyState[]>([]);
  const [capturing, setCapturing] = useState<keyof Hotkeys | null>(null);
  useEffect(() => {
    void backend().invoke("hotkey_status").then(setStatus);
  }, [settings.hotkeys]);
  useAppEvents((e) => {
    if (e.type === "hotkeyStatus") setStatus(e.hotkeys);
  });
  return (
    <>
      <h2 className="page">{t("settings.tab.shortcuts")}</h2>
      <Section lead={t("shortcuts.lead")}>
        {HOTKEY_ORDER.map((action) => {
          const ok = status.find((s) => s.action === action)?.ok ?? true;
          return (
            <Field key={action} label={t(`shortcuts.${action}`)}>
              <span className="kbd" style={!ok ? { borderColor: "var(--err)", color: "var(--err)" } : undefined}>
                {settings.hotkeys[action]}
              </span>
              {!ok && <Badge tone="err">{errorTitle("MC-HK-001")}</Badge>}
              <button
                className="btn btn-small"
                aria-pressed={capturing === action}
                onClick={() => setCapturing(capturing === action ? null : action)}
                onKeyDown={(e) => {
                  if (capturing !== action) return;
                  e.preventDefault();
                  if (e.key === "Escape") {
                    setCapturing(null);
                    return;
                  }
                  const acc = acceleratorFromEvent(e.nativeEvent);
                  if (acc) {
                    setCapturing(null);
                    void update((s) => {
                      s.hotkeys[action] = acc;
                      return s;
                    });
                  }
                }}
              >
                {capturing === action ? t("shortcuts.pressKeys") : t("shortcuts.change")}
              </button>
            </Field>
          );
        })}
        <div className="row" style={{ padding: "10px 0" }}>
          <span className="muted">{t("shortcuts.why")}</span>
          <span className="spacer" />
          <button
            className="btn btn-small"
            onClick={() =>
              void update((s) => {
                s.hotkeys = { toggleCapture: "Ctrl+Alt+Space", cyclePanel: "Ctrl+Alt+H", focusPrev: "Ctrl+Alt+[", focusNext: "Ctrl+Alt+]", markAnswered: "Ctrl+Alt+Enter" };
                return s;
              })
            }
          >
            {t("shortcuts.restoreDefaults")}
          </button>
        </div>
      </Section>
    </>
  );
}

function DiagnosticsTab({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const info = useAppInfo();
  const levels = useLevels();
  const [recent, setRecent] = useState<MeetCueEvent[]>([]);
  const [state, setState] = useState<string>("idle");
  const [preview, setPreview] = useState<string[] | null>(null);
  const [exported, setExported] = useState<string | null>(null);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    void backend()
      .onEvent((e) => {
        if (e.type === "sessionState") setState(e.state);
        if (e.type === "levels" || (e.type === "transcript" && !e.isFinal)) return;
        setRecent((r) => [e, ...r].slice(0, 10));
      })
      .then((u) => {
        if (alive) unlisten = u;
        else u();
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return (
    <>
      <h2 className="page">{t("settings.tab.diagnostics")}</h2>
      <Section title={t("diag.liveTitle")}>
        <Field label={t("overlay.sourceMe")}>
          <LevelBar db={levels.me} label={t("overlay.sourceMe")} />
        </Field>
        <Field label={t("overlay.sourceRemote")}>
          <LevelBar db={levels.remote} label={t("overlay.sourceRemote")} />
        </Field>
        <Field label={t("diag.session")}>
          <Badge tone={state === "running" ? "ok" : undefined}>{t(`status.${state}`)}</Badge>
        </Field>
        <Field label={t("diag.recent")}>
          <ol style={{ margin: 0, paddingLeft: 18, fontSize: 13, width: "100%" }}>
            {recent.map((e, i) => (
              <li key={i}>
                <EventLine e={e} />
              </li>
            ))}
            {!recent.length && <li className="muted">{t("diag.noEvents")}</li>}
          </ol>
        </Field>
      </Section>
      <Section title={t("diag.exportTitle")} lead={t("diag.exportLead")}>
        {preview && (
          <ul style={{ margin: "0 0 8px", fontSize: 13 }}>
            {preview.map((p) => (
              <li key={p} className="mono">
                {p}
              </li>
            ))}
          </ul>
        )}
        <div className="row" style={{ paddingBottom: 12 }}>
          {!preview ? (
            <button className="btn" onClick={() => void backend().invoke("diagnostics_preview").then(setPreview)}>
              {t("diag.showContents")}
            </button>
          ) : (
            <button className="btn btn-primary" onClick={() => void backend().invoke("export_diagnostics").then(setExported)}>
              {t("diag.export")}
            </button>
          )}
          {exported && <span className="muted">{t("diag.exported", { path: exported })}</span>}
        </div>
        <Switch
          checked={settings.diagnostics.debugLogging}
          label={t("diag.debugLogging")}
          description={t("diag.debugLoggingHint")}
          onChange={(v) =>
            void update((s) => {
              s.diagnostics.debugLogging = v;
              return s;
            })
          }
        />
      </Section>
      <Section title={t("diag.aboutTitle")}>
        <Field label={t("diag.version")}>
          <span className="mono">{info?.version}</span>
        </Field>
        <Field label={t("diag.configDir")}>
          <span className="mono" style={{ wordBreak: "break-all" }}>
            {info?.configDir}
          </span>
          {info?.portable && <Badge>{t("diag.portable")}</Badge>}
        </Field>
      </Section>
    </>
  );
}

function EventLine({ e }: { e: MeetCueEvent }) {
  const { t } = useTranslation();
  switch (e.type) {
    case "latencySample":
      return (
        <span className="mono">
          {t("diag.latencyLine", { total: e.totalMs, asr: e.asrMs, detect: e.detectMs, generate: e.generateMs })}
        </span>
      );
    case "error":
      return (
        <span>
          {errorTitle(e.code)} <span className="mono muted">{e.code}</span>
        </span>
      );
    case "questionNew":
      return <span>{t("diag.questionLine", { short: e.short })}</span>;
    case "usage":
      return (
        <span className="mono">
          {e.provider}/{e.model} · {usd(e.costMicros)}
        </span>
      );
    default:
      return <span className="mono muted">{e.type}</span>;
  }
}
