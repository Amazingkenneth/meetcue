import { useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { Settings } from "../bindings/Settings";
import type { VerifyResult } from "../bindings/VerifyResult";
import { Badge, Field } from "../design/components";
import { errorTitle } from "../util/errors";
import { type ProviderMeta } from "./providers";

/** Key entry for one provider. The key itself never comes back from the backend. */
export function KeyField({
  provider,
  configured,
  settings,
  update,
  onChanged,
}: {
  provider: ProviderMeta;
  configured: boolean;
  settings: Settings;
  update: (f: (s: Settings) => Settings) => Promise<void>;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<VerifyResult | null>(null);
  const inputId = `key-${provider.id}`;

  const saveAndVerify = async () => {
    setBusy(true);
    try {
      if (value.trim()) {
        await backend().invoke("set_credential", { key: provider.id, value: value.trim() });
        setValue("");
        onChanged();
      }
      setResult(await backend().invoke("verify_provider", { provider: provider.id }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {provider.needs === "baseUrl" && (
        <Field label={t("providers.baseUrl")} htmlFor={`url-${provider.id}`} hint={t("providers.baseUrlHint")}>
          <input
            id={`url-${provider.id}`}
            className="input-wide"
            type="url"
            placeholder="https://host/v1"
            value={settings.providers.openaiBaseUrl ?? ""}
            onChange={(e) =>
              void update((s) => {
                s.providers.openaiBaseUrl = e.target.value.trim() || null;
                return s;
              })
            }
          />
        </Field>
      )}
      <Field
        label={t("providers.keyFor", { provider: provider.name })}
        htmlFor={inputId}
        hint={
          <>
            {t("providers.keyHint")}{" "}
            {provider.consoleUrl && (
              <button className="btn-link" onClick={() => void backend().invoke("open_url", { url: provider.consoleUrl! })}>
                {t("providers.getKey")}
              </button>
            )}
          </>
        }
      >
        <input
          id={inputId}
          className="input-wide"
          type="password"
          autoComplete="off"
          spellCheck={false}
          placeholder={configured ? t("providers.keySaved") : t("providers.keyPlaceholder")}
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
        <button className="btn" disabled={busy || (!value.trim() && !configured && !provider.keyOptional)} onClick={() => void saveAndVerify()}>
          {value.trim() ? t("providers.saveVerify") : t("common.verify")}
        </button>
        {configured && (
          <button
            className="btn"
            onClick={() =>
              void backend()
                .invoke("delete_credential", { key: provider.id })
                .then(() => {
                  setResult(null);
                  onChanged();
                })
            }
          >
            {t("common.remove")}
          </button>
        )}
        {result?.ok && <Badge tone="ok">{t("common.verified")}</Badge>}
        {result && !result.ok && (
          <Badge tone="err">
            {errorTitle(result.code ?? "")} · {result.code}
          </Badge>
        )}
        {!result && <Badge tone={configured ? "ok" : undefined}>{configured ? t("common.configured") : t("common.notConfigured")}</Badge>}
      </Field>
    </>
  );
}
