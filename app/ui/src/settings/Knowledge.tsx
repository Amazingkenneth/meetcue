import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { FileFailure } from "../bindings/FileFailure";
import type { KnowledgePreview } from "../bindings/KnowledgePreview";
import type { PromptSettings } from "../bindings/PromptSettings";
import type { Settings } from "../bindings/Settings";
import { Badge, Field, Section } from "../design/components";
import { errorFix, errorTitle } from "../util/errors";

type Update = (f: (s: Settings) => Settings) => Promise<void>;

/** Material up to this size is sent whole; longer material is searched per question (`materials::SUMMARY_CHARS`). */
export const WHOLE_CHARS = 3_000;

/** Placeholders each prompt needs; a missing one means that information never reaches the model. */
export const PLACEHOLDERS: Record<keyof PromptSettings, string[]> = {
  hintsSystem: ["{role}", "{materials}", "{output_language}"],
  hintsUser: ["{knowledge}", "{transcript}", "{question}"],
};

export function missingPlaceholders(kind: keyof PromptSettings, text: string): string[] {
  return PLACEHOLDERS[kind].filter((p) => !text.includes(p));
}

export function KnowledgeSection({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const [failures, setFailures] = useState<FileFailure[]>([]);
  const [question, setQuestion] = useState("");
  const [preview, setPreview] = useState<KnowledgePreview | null>(null);
  const files = settings.hints.materials;
  const total = files.reduce((n, m) => n + [...m.text].length, 0);

  const add = async () => {
    const result = await backend().invoke("import_materials");
    setFailures(result.failures);
    if (result.materials.length)
      await update((s) => {
        // Adding a file again replaces the old copy.
        const paths = new Set(result.materials.map((m) => m.path));
        s.hints.materials = [...s.hints.materials.filter((m) => !paths.has(m.path)), ...result.materials];
        return s;
      });
    setPreview(null);
  };

  const test = async () => {
    if (question.trim()) setPreview(await backend().invoke("preview_knowledge", { question: question.trim() }));
  };

  return (
    <Section title={t("knowledge.title")} lead={t("knowledge.lead")}>
      <Field label={t("knowledge.files")} hint={t("knowledge.filesHint")}>
        <div style={{ display: "grid", gap: 6, width: "100%" }}>
          {files.map((m, i) => (
            <div className="row" key={m.path + i}>
              <span title={m.path}>{m.name}</span>
              <span className="muted">{t("hints.chars", { count: [...m.text].length })}</span>
              <span className="spacer" />
              <button
                className="btn btn-small"
                onClick={() =>
                  void update((s) => {
                    s.hints.materials.splice(i, 1);
                    return s;
                  }).then(() => setPreview(null))
                }
              >
                {t("common.remove")}
              </button>
            </div>
          ))}
          {files.length === 0 && <span className="muted">{t("knowledge.empty")}</span>}
          {failures.length > 0 && (
            <div className="note note-err" role="alert">
              {failures.map((f) => (
                <p key={f.name} style={{ margin: "2px 0" }}>
                  <strong>{f.name}</strong>: {errorTitle(f.error.code)} ({f.error.detail})
                </p>
              ))}
              <p style={{ margin: "2px 0" }}>{errorFix(failures[0].error.code)}</p>
            </div>
          )}
          <div className="row">
            <button className="btn btn-small" onClick={() => void add()}>
              {t("knowledge.add")}
            </button>
            {files.length > 0 && (
              <span className="muted" style={{ fontSize: 12 }}>
                {total <= WHOLE_CHARS ? t("knowledge.modeWhole") : t("knowledge.modeSearch")}
              </span>
            )}
          </div>
        </div>
      </Field>
      {files.length > 0 && (
        <Field label={t("knowledge.try")} htmlFor="kb-try" hint={t("knowledge.tryHint")}>
          <input
            id="kb-try"
            type="text"
            className="input-wide"
            value={question}
            placeholder={t("knowledge.tryPlaceholder")}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void test();
            }}
          />
          <button className="btn btn-small" onClick={() => void test()} disabled={!question.trim()}>
            {t("knowledge.tryButton")}
          </button>
          {preview && <PreviewResult preview={preview} />}
        </Field>
      )}
    </Section>
  );
}

function PreviewResult({ preview }: { preview: KnowledgePreview }) {
  const { t } = useTranslation();
  return (
    <div style={{ display: "grid", gap: 6, width: "100%" }} aria-live="polite">
      {!preview.searched ? (
        <p className="muted" style={{ margin: 0 }}>
          {t("knowledge.allSent")}
        </p>
      ) : preview.passages.length === 0 ? (
        <p className="muted" style={{ margin: 0 }}>
          {t("knowledge.noMatch")}
        </p>
      ) : (
        preview.passages.map((p, i) => (
          <div key={i} className="note">
            <strong>{p.source}</strong>
            <p style={{ margin: "4px 0 0", whiteSpace: "pre-wrap" }}>{p.text}</p>
          </div>
        ))
      )}
      <details>
        <summary>{t("knowledge.excerpt")}</summary>
        <pre className="prompt-text">{preview.excerpt}</pre>
      </details>
    </div>
  );
}

export function PromptsSection({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const [defaults, setDefaults] = useState<PromptSettings | null>(null);
  useEffect(() => {
    void backend().invoke("default_prompts").then(setDefaults);
  }, []);
  if (!defaults) return null;
  const field = (kind: keyof PromptSettings) => {
    const custom = settings.hints.prompts[kind];
    const value = custom || defaults[kind];
    const missing = missingPlaceholders(kind, value);
    const id = `prompt-${kind}`;
    return (
      <Field
        label={
          <>
            {t(`prompts.${kind}`)} {custom ? <Badge tone="warn">{t("prompts.custom")}</Badge> : <Badge>{t("prompts.builtIn")}</Badge>}
          </>
        }
        htmlFor={id}
        hint={t(`prompts.${kind}Hint`, { placeholders: PLACEHOLDERS[kind].join(" ") })}
      >
        <textarea
          id={id}
          className="prompt-text"
          rows={kind === "hintsSystem" ? 14 : 8}
          spellCheck={false}
          value={value}
          onChange={(e) => {
            const text = e.target.value;
            void update((s) => {
              // Matching the built-in text again means "use the built-in prompt", so later improvements apply.
              s.hints.prompts[kind] = text === defaults[kind] ? "" : text;
              return s;
            });
          }}
        />
        {missing.length > 0 && (
          <p className="note note-warn" role="alert" style={{ margin: 0, width: "100%" }}>
            {t("prompts.missing", { placeholders: missing.join(" ") })}
          </p>
        )}
        {custom && (
          <button
            className="btn btn-small"
            onClick={() =>
              void update((s) => {
                s.hints.prompts[kind] = "";
                return s;
              })
            }
          >
            {t("prompts.reset")}
          </button>
        )}
      </Field>
    );
  };
  return (
    <Section title={t("prompts.title")} lead={t("prompts.lead")}>
      {field("hintsSystem")}
      {field("hintsUser")}
    </Section>
  );
}
