import { useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { Settings } from "../bindings/Settings";
import type { VerifyResult } from "../bindings/VerifyResult";
import { Badge } from "../design/components";
import { errorTitle } from "../util/errors";
import { type ProviderMeta } from "./providers";

/** One compact row per provider. The key itself never comes back from the backend. */
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

  const status = result ? (
    result.ok ? (
      <Badge tone="ok">{t("common.verified")}</Badge>
    ) : (
      <Badge tone="err">
        {errorTitle(result.code ?? "")} · {result.code}
      </Badge>
    )
  ) : (
    <Badge tone={configured ? "ok" : undefined}>{configured ? t("common.configured") : t("common.notConfigured")}</Badge>
  );

  return (
    <div className="key-row">
      <label className="key-name" htmlFor={provider.needs === "baseUrl" ? `url-${provider.id}` : inputId}>
        {provider.name}
      </label>
      <div className="key-inputs">
        {provider.needs === "baseUrl" && (
          <input
            id={`url-${provider.id}`}
            type="url"
            aria-label={t("providers.baseUrl")}
            placeholder={t("providers.baseUrlPlaceholder")}
            value={settings.providers.openaiBaseUrl ?? ""}
            onChange={(e) =>
              void update((s) => {
                s.providers.openaiBaseUrl = e.target.value.trim() || null;
                return s;
              })
            }
          />
        )}
        <input
          id={inputId}
          type="password"
          autoComplete="off"
          spellCheck={false}
          aria-label={t("providers.keyFor", { provider: provider.name })}
          placeholder={
            configured ? t("providers.keySaved") : provider.keyOptional ? t("providers.keyOptional") : t("providers.keyPlaceholder")
          }
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
      </div>
      <div className="key-actions">
        <button
          className="btn btn-small"
          disabled={busy || (!value.trim() && !configured && !provider.keyOptional)}
          onClick={() => void saveAndVerify()}
        >
          {value.trim() ? t("providers.saveVerify") : t("common.verify")}
        </button>
        {status}
        {configured && (
          <button
            className="btn-link"
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
        {provider.consoleUrl && !configured && (
          <button className="btn-link" onClick={() => void backend().invoke("open_url", { url: provider.consoleUrl! })}>
            {t("providers.getKey")} ↗
          </button>
        )}
      </div>
    </div>
  );
}
