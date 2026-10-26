import { useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import { Section } from "../design/components";
import { allChosen, ModelsPanel, SLOTS } from "../settings/ModelPicker";
import { ProbePanel } from "../settings/Probe";
import { AudioFields, LanguageField, useCredentials } from "../settings/Settings";
import { providerName, providersInUse, reachable } from "../settings/providers";
import { useDraft } from "../settings/useDraft";

const STEPS = ["language", "audio", "models", "probe", "done"] as const;
type Step = (typeof STEPS)[number];

export function Wizard() {
  const { t } = useTranslation();
  const { settings, update } = useDraft();
  const [step, setStep] = useState<Step>("language");
  const [credentials, refresh] = useCredentials();
  if (!settings) return <p className="app-body muted">{t("common.loading")}</p>;

  const idx = STEPS.indexOf(step);
  const chosen = allChosen(settings);
  const keysReady = providersInUse(settings).every((p) => reachable(settings, p) && (credentials[p.id] || p.keyOptional));
  const canNext = step === "models" ? chosen : true;

  const finish = async () => {
    await update((s) => {
      s.general.wizardCompleted = true;
      return s;
    });
    await backend().invoke("set_panel_mode", { mode: "full" });
    await backend().closeWindow();
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
        <Section title={t("wizard.step.models")} lead={t("wizard.modelsLead")}>
          <ModelsPanel settings={settings} update={update} credentials={credentials} onCredentialsChanged={refresh} />
          {!chosen && <p className="note note-warn">{t("wizard.chooseAll")}</p>}
          {chosen && !keysReady && <p className="note note-warn">{t("wizard.keysMissing")}</p>}
        </Section>
      )}
      {step === "probe" && (
        <Section title={t("wizard.step.probe")}>
          <ProbePanel settings={settings} credentials={credentials} update={update} />
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
