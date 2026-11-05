import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { ChatGptAccount } from "../bindings/ChatGptAccount";
import type { Settings } from "../bindings/Settings";
import type { VerifyResult } from "../bindings/VerifyResult";
import { Badge } from "../design/components";
import { errorTitle } from "../util/errors";
import { type ProviderMeta } from "./providers";

/** One compact row per provider: its base URL where it takes one, and its key.
 * The key itself never comes back from the backend. */
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
  if (provider.signIn) return <SignInField provider={provider} configured={configured} settings={settings} onChanged={onChanged} />;
  return <KeyInputs provider={provider} configured={configured} settings={settings} update={update} onChanged={onChanged} />;
}

function KeyInputs({
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
  const urlId = `url-${provider.id}`;

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
      <label className="key-name" htmlFor={provider.baseUrl ? urlId : inputId}>
        {provider.name}
      </label>
      <div className="key-inputs">
        {provider.baseUrl && (
          <input
            id={urlId}
            type="url"
            spellCheck={false}
            aria-label={t("providers.baseUrlFor", { provider: provider.name })}
            placeholder={
              provider.defaultBaseUrl
                ? t("providers.baseUrlDefault", { url: provider.defaultBaseUrl })
                : t("providers.baseUrlPlaceholder")
            }
            value={settings.providers.baseUrls[provider.id] ?? ""}
            onChange={(e) =>
              void update((s) => {
                const url = e.target.value.trim();
                if (url) s.providers.baseUrls[provider.id] = url;
                else delete s.providers.baseUrls[provider.id];
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
            configured
              ? t("providers.keySaved")
              : provider.keyOptional
                ? t("providers.keyOptional")
                : provider.keyFormat
                  ? t(provider.keyFormat)
                  : t("providers.keyPlaceholder")
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

/** Experimental: "Sign in with ChatGPT" in the browser, instead of a key.
 * Tokens stay in the credential store; only the email and plan come back. */
function SignInField({
  provider,
  configured,
  settings,
  onChanged,
}: {
  provider: ProviderMeta;
  configured: boolean;
  settings: Settings;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [account, setAccount] = useState<ChatGptAccount | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<VerifyResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const enabled = settings.providers.chatgptSubscription;

  useEffect(() => {
    if (!configured) {
      setAccount(null);
      return;
    }
    void backend().invoke("chatgpt_account").then(setAccount);
  }, [configured]);

  const signIn = async () => {
    setWaiting(true);
    setError(null);
    setResult(null);
    try {
      setAccount(await backend().invoke("chatgpt_sign_in"));
      onChanged();
    } catch (e) {
      const code = (e as { code?: string })?.code;
      const detail = (e as { detail?: string })?.detail;
      if (detail !== "sign-in cancelled") setError(code ? `${errorTitle(code)} · ${code}` : String(e));
    } finally {
      setWaiting(false);
    }
  };

  const verify = async () => {
    setBusy(true);
    try {
      setResult(await backend().invoke("verify_provider", { provider: provider.id }));
    } finally {
      setBusy(false);
    }
  };

  const who = account?.email ? (account.plan ? `${account.email} · ${account.plan}` : account.email) : t("chatgpt.signedIn");

  return (
    <div className="key-row">
      <span className="key-name">{provider.name}</span>
      <div className="key-inputs">
        <p className="note note-warn" style={{ margin: 0 }}>
          {t("chatgpt.warning")}
        </p>
        {!enabled && <p className="muted">{t("chatgpt.turnedOff")}</p>}
      </div>
      <div className="key-actions">
        {configured ? (
          <>
            <Badge tone={result && !result.ok ? "err" : "ok"}>
              {result && !result.ok ? `${errorTitle(result.code ?? "")} · ${result.code}` : result?.ok ? t("common.verified") : who}
            </Badge>
            <button className="btn btn-small" disabled={busy || !enabled} onClick={() => void verify()}>
              {t("common.verify")}
            </button>
            <button
              className="btn-link"
              onClick={() =>
                void backend()
                  .invoke("chatgpt_sign_out")
                  .then(() => {
                    setAccount(null);
                    setResult(null);
                    onChanged();
                  })
              }
            >
              {t("chatgpt.signOut")}
            </button>
          </>
        ) : waiting ? (
          <>
            <span className="muted">{t("chatgpt.waiting")}</span>
            <button className="btn-link" onClick={() => void backend().invoke("chatgpt_cancel_sign_in")}>
              {t("common.cancel")}
            </button>
          </>
        ) : (
          <button className="btn btn-small btn-primary" disabled={!enabled} onClick={() => void signIn()}>
            {t("chatgpt.signIn")}
          </button>
        )}
        {error && <Badge tone="err">{error}</Badge>}
      </div>
    </div>
  );
}
