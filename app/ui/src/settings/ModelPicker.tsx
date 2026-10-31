import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { ModelSlot } from "../backend/types";
import type { ModelInfo } from "../bindings/ModelInfo";
import type { PriceEntry } from "../bindings/PriceEntry";
import type { Settings } from "../bindings/Settings";
import { Field } from "../design/components";
import { KeyField } from "./Keys";
import { ModelCombo, usePriceTable, useUnitPrice, type ListState } from "./ModelCombo";
import { usd } from "../util/format";
import { modelOptions } from "../util/models";
import { baseUrlOf, providerMeta, providersFor, providersInUse, reachable, type ProviderMeta } from "./providers";

type Update = (f: (s: Settings) => Settings) => Promise<void>;

export const SLOTS: ModelSlot[] = ["asr", "detect", "hints"];

/** Whether every task has a provider and a model name. */
export function allChosen(settings: Settings): boolean {
  return SLOTS.every((s) => settings.providers[s].provider && settings.providers[s].model);
}

/** Choosing another provider clears the model, since model names don't carry over. */
export function chooseProvider(s: Settings, slot: ModelSlot, provider: string | null): Settings {
  const prev = s.providers[slot];
  if (prev.provider === provider) return s;
  s.providers[slot] = { ...prev, provider, model: null, endpoint: { mode: "auto" } };
  return s;
}

/**
 * The same form in setup and in Settings: a provider and a model name for each
 * task, then the address and key of every provider in use.
 */
export function ModelsPanel({
  settings,
  update,
  credentials,
  onCredentialsChanged,
  asrExtra,
}: {
  settings: Settings;
  update: Update;
  credentials: Record<string, boolean>;
  onCredentialsChanged: () => void;
  asrExtra?: ReactNode;
}) {
  const { t } = useTranslation();
  const inUse = providersInUse(settings);
  return (
    <div className="models">
      {SLOTS.map((slot) => (
        <div key={slot} className="model-slot">
          <h3>{t(`providers.${slot}Title`)}</h3>
          <p className="muted">{t(`providers.${slot}Lead`)}</p>
          <ModelPicker slot={slot} settings={settings} update={update} credentials={credentials} />
          {slot === "asr" && asrExtra}
        </div>
      ))}
      <div id="keys" />
      <div className="model-slot">
        <h3>{t("providers.keysTitle")}</h3>
        <p className="muted">{t("providers.keysLead")}</p>
        {inUse.map((p) => (
          <KeyField
            key={p.id}
            provider={p}
            configured={!!credentials[p.id]}
            settings={settings}
            update={update}
            onChanged={onCredentialsChanged}
          />
        ))}
        {!inUse.length && <p className="muted">{t("providers.keysNone")}</p>}
      </div>
    </div>
  );
}

/** Provider and model for one task. The model comes from a list with prices, or is typed freely. */
export function ModelPicker({
  slot,
  settings,
  update,
  credentials,
}: {
  slot: ModelSlot;
  settings: Settings;
  update: Update;
  credentials: Record<string, boolean>;
}) {
  const { t } = useTranslation();
  const choice = settings.providers[slot];
  const meta = providerMeta(choice.provider);
  const { models: reported, state } = useModelList(meta, settings, credentials);
  const table = usePriceTable();
  const unitPrice = useUnitPrice();
  const providerId = `provider-${slot}`;
  const modelId = `model-${slot}`;
  const options = useMemo(
    () =>
      meta
        ? modelOptions(meta.id, slot, table?.entries ?? [], settings.providers.pricingOverrides, reported, settings.providers.estimate)
        : [],
    [meta, slot, table, settings.providers.pricingOverrides, reported, settings.providers.estimate],
  );
  const model = choice.model ?? "";
  const chosen = options.find((o) => o.id === model) ?? null;
  const chosenUnit = chosen?.price ? unitPrice(chosen.price) : null;

  return (
    <div className="model-fields">
      <Field label={t("providers.provider")} htmlFor={providerId}>
        <select
          id={providerId}
          value={choice.provider ?? ""}
          onChange={(e) => void update((s) => chooseProvider(s, slot, e.target.value || null))}
        >
          <option value="">{t("providers.chooseProvider")}</option>
          {providersFor(slot)
            .filter((p) => p.available || p.id === choice.provider)
            .map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
        </select>
        {meta && <PricingLink meta={meta} />}
      </Field>
      <Field label={t("providers.model")} htmlFor={modelId}>
        <ModelCombo
          id={modelId}
          slot={slot}
          value={model}
          options={options}
          listState={state}
          providerName={meta?.name ?? ""}
          disabled={!meta}
          placeholder={meta?.modelExample ? t("providers.modelExample", { model: meta.modelExample }) : t("providers.modelPlaceholder")}
          onChange={(m) =>
            void update((s) => {
              s.providers[slot] = { ...s.providers[slot], model: m };
              return s;
            })
          }
        />
        {meta && model && (
          <span className="model-price">
            {chosen?.perHour != null ? (
              <>
                <strong>{t("models.perMeetingHour", { price: usd(chosen.perHour) })}</strong>
                {chosenUnit && <span className="muted"> · {chosenUnit}</span>}
              </>
            ) : (
              <span className="muted">{t("models.noPriceChosen")}</span>
            )}
          </span>
        )}
      </Field>
    </div>
  );
}

/** Models the provider itself reports, once it can be reached. */
function useModelList(
  meta: ProviderMeta | undefined,
  settings: Settings,
  credentials: Record<string, boolean>,
): { models: ModelInfo[]; state: ListState } {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [state, setState] = useState<ListState>("none");
  const id = meta?.id;
  const ready = !!meta && reachable(settings, meta) && (!!credentials[meta.id] || !!meta.keyOptional);
  const url = id ? baseUrlOf(settings, id) : null;
  useEffect(() => {
    setModels([]);
    if (!id) {
      setState("none");
      return;
    }
    if (!ready) {
      setState("needsKey");
      return;
    }
    setState("loading");
    let alive = true;
    void backend()
      .invoke("list_models", { provider: id })
      .then((m) => {
        if (!alive) return;
        setModels(m);
        setState("loaded");
      })
      .catch(() => alive && setState("none"));
    return () => {
      alive = false;
    };
  }, [id, ready, url]);
  return { models, state };
}

/** A link to the provider's own price page. */
export function PricingLink({ meta }: { meta: ProviderMeta }) {
  const { t } = useTranslation();
  if (!meta.pricingUrl) return <span className="muted">{t("providers.pricingElsewhere")}</span>;
  const url = meta.pricingUrl;
  return (
    <button className="btn-link" onClick={() => void backend().invoke("open_url", { url })}>
      {t("providers.officialPricing")} ↗
    </button>
  );
}

export function PriceEditor({
  slot,
  provider,
  model,
  initial,
  onSave,
  onCancel,
}: {
  slot: ModelSlot;
  provider: string;
  model: string;
  initial?: PriceEntry | null;
  onSave: (e: PriceEntry) => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [a, setA] = useState(String((slot === "asr" ? initial?.audio : initial?.input) ?? ""));
  const [b, setB] = useState(String(initial?.output ?? ""));
  const num = (s: string) => (s.trim() === "" ? undefined : Number(s));
  return (
    <div className="note" role="group" aria-label={t("providers.setPriceFor", { model })}>
      <div className="row">
        <strong>{t("providers.setPriceFor", { model })}</strong>
      </div>
      <div className="row" style={{ marginTop: 8 }}>
        {slot === "asr" ? (
          <label className="row">
            {t("providers.perHourUsd")}
            <input type="number" min="0" step="0.0001" value={a} onChange={(e) => setA(e.target.value)} />
          </label>
        ) : (
          <>
            <label className="row">
              {t("providers.inputPerMillion")}
              <input type="number" min="0" step="0.0001" value={a} onChange={(e) => setA(e.target.value)} />
            </label>
            <label className="row">
              {t("providers.outputPerMillion")}
              <input type="number" min="0" step="0.0001" value={b} onChange={(e) => setB(e.target.value)} />
            </label>
          </>
        )}
        <button
          className="btn btn-primary btn-small"
          onClick={() =>
            onSave({
              provider,
              model,
              label: model,
              unit: slot === "asr" ? "per_audio_hour" : "per_million_tokens",
              ...(slot === "asr" ? { audio: num(a) } : { input: num(a), output: num(b) }),
              source: "user",
              verified: new Date().toISOString().slice(0, 10),
            })
          }
        >
          {t("common.save")}
        </button>
        <button className="btn btn-small" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}
