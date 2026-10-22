import { useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { ModelSlot } from "../backend/types";
import type { ProbeReport } from "../bindings/ProbeReport";
import { Section } from "../design/components";
import { KeyField } from "../settings/Keys";
import { ModelPicker } from "../settings/ModelPicker";
import { ProbePanel } from "../settings/Probe";
import { AudioFields, LanguageField, useCredentials, usePricing } from "../settings/Settings";
import { PROVIDERS, providerName } from "../settings/providers";
import { useDraft } from "../settings/useDraft";

const STEPS = ["language", "audio", "models", "keys", "probe", "done"] as const;
type Step = (typeof STEPS)[number];
const SLOTS: ModelSlot[] = ["asr", "detect", "hints"];

export function Wizard() {
  const { t } = useTranslation();
  const { settings, update } = useDraft();
  const [step, setStep] = useState<Step>("language");
  const [credentials, refresh] = useCredentials();
  const table = usePricing();
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  if (!settings) return <p className="app-body muted">{t("common.loading")}</p>;

  const idx = STEPS.indexOf(step);
  const chosen = SLOTS.every((s) => settings.providers[s].provider && settings.providers[s].model);
  const neededProviders = [...new Set(SLOTS.map((s) => settings.providers[s].provider).filter(Boolean) as string[])];
  const keysReady = neededProviders.every((p) => credentials[p] || PROVIDERS.find((x) => x.id === p)?.keyOptional);
  const canNext = step === "models" ? chosen : true;

  const finish = async () => {
    await update((s) => {
      s.general.wizardCompleted = true;
      return s;
    });
    await backend().invoke("set_panel_mode", { mode: "full" });
    window.close();
  };

  return (
    <div className="wizard">
      <h1 style={{ margin: "0 0 4px", fontSize: 22 }}>{t("wizard.title")}</h1>
      <p className="muted" style={{ marginTop: 0 }}>
        {t("wizard.lead")}
      </p>
      <ol className="steps" aria-label={t("wizard.progress")}>
        {STEPS.map((s, i) => (
          <li key={s} aria-current={s === step ? "step" : undefined} data-done={i < idx}>
            {i + 1}. {t(`wizard.step.${s}`)}
          </li>
        ))}
      </ol>

      {step === "language" && (
        <Section title={t("wizard.step.language")} lead={t("wizard.languageLead")}>
          <LanguageField settings={settings} update={update} />
        </Section>
      )}
      {step === "audio" && (
        <Section title={t("wizard.step.audio")} lead={t("wizard.audioLead")}>
          <AudioFields settings={settings} update={update} />
        </Section>
      )}
      {step === "models" && (
        <>
          <p className="note">{t("wizard.modelsLead")}</p>
          {SLOTS.map((slot) => (
            <Section key={slot} title={t(`providers.${slot}Title`)} lead={t(`providers.${slot}Lead`)}>
              <ModelPicker slot={slot} settings={settings} update={update} table={table} probe={probe} credentials={credentials} />
            </Section>
          ))}
          {!chosen && <p className="note note-warn">{t("wizard.chooseAll")}</p>}
        </>
      )}
      {step === "keys" && (
        <Section title={t("wizard.step.keys")} lead={t("wizard.keysLead")}>
          {neededProviders.map((id) => {
            const p = PROVIDERS.find((x) => x.id === id);
            return p ? (
              <KeyField key={id} provider={p} configured={!!credentials[id]} settings={settings} update={update} onChanged={refresh} />
            ) : null;
          })}
          {!keysReady && <p className="note note-warn">{t("wizard.keysMissing")}</p>}
        </Section>
      )}
      {step === "probe" && (
        <Section title={t("wizard.step.probe")}>
          <ProbePanel settings={settings} credentials={credentials} update={update} onReport={setProbe} />
        </Section>
      )}
      {step === "done" && (
        <Section title={t("wizard.doneTitle")}>
          <p>{t("wizard.doneBody", { key: settings.hotkeys.toggleCapture })}</p>
          <ul>
            {SLOTS.map((s) => (
              <li key={s}>
                {t(`wizard.slot.${s}`)}: {providerName(settings.providers[s].provider)} · {settings.providers[s].model}
              </li>
            ))}
          </ul>
          <p className="muted">{t("wizard.doneDemo")}</p>
        </Section>
      )}

      <div className="wizard-nav">
        {step === "probe" && (
          <button className="btn" onClick={() => setStep("done")}>
            {t("common.skip")}
          </button>
        )}
        <span className="spacer" />
        {idx > 0 && (
          <button className="btn" onClick={() => setStep(STEPS[idx - 1])}>
            {t("common.back")}
          </button>
        )}
        {step !== "done" ? (
          <button className="btn btn-primary" disabled={!canNext} onClick={() => setStep(STEPS[idx + 1])}>
            {t("common.next")}
          </button>
        ) : (
          <button className="btn btn-primary" onClick={() => void finish()}>
            {t("common.finish")}
          </button>
        )}
      </div>
    </div>
  );
}
