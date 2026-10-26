import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { backend } from "../backend";
import type { HintLevel } from "../bindings/HintLevel";
import type { PracticeReportSummary } from "../bindings/PracticeReportSummary";
import type { PracticeScript } from "../bindings/PracticeScript";
import type { Settings } from "../bindings/Settings";
import type { Voice } from "../bindings/Voice";
import { Field, Section, Switch } from "../design/components";
import { LevelBar, useLevels } from "../settings/Settings";
import { useDraft } from "../settings/useDraft";
import { useAppEvents } from "../state/hooks";
import {
  answerTurns,
  editItem,
  keepFirst,
  moveItem,
  removeItem,
  setMeSpeaker,
  shuffleItems,
  usePracticeProgress,
  type PracticeProgress,
} from "../state/practice";
import { errorFix, errorTitle } from "../util/errors";
import { seconds } from "../util/format";
import "./practice.css";

type Update = (f: (s: Settings) => Settings) => Promise<void>;

function errorCode(e: unknown): string {
  return String((e as { code?: string })?.code ?? e);
}

const STEPS = ["sound", "questions", "check", "practise"] as const;
type Step = (typeof STEPS)[number];

export function Practice() {
  const { t } = useTranslation();
  const { settings, update } = useDraft();
  const [script, setScript] = useState<PracticeScript | null>(null);
  const [busy, setBusy] = useState<null | "import" | "classify" | "start">(null);
  const [error, setError] = useState<string | null>(null);
  const [started, setStarted] = useState(false);
  const [finished, setFinished] = useState(false);
  const [step, setStep] = useState<Step>("sound");
  const progress = usePracticeProgress();
  useAppEvents((e) => {
    if (e.type === "practiceFinished") {
      setStarted(false);
      setFinished(true);
      setScript(null);
      setStep("questions");
    }
  });

  const run = async (kind: "import" | "classify" | "start", f: () => Promise<void>) => {
    setBusy(kind);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorCode(e));
    } finally {
      setBusy(null);
    }
  };

  const chose = (s: PracticeScript) => {
    setScript(s);
    setFinished(false);
    setStep("check");
  };

  const importFile = () =>
    run("import", async () => {
      const s = await backend().invoke("practice_import");
      if (s) chose(s);
    });

  const loadSample = (name: string) =>
    run("import", async () => chose(await backend().invoke("practice_sample", { name })));

  if (!settings) return <p className="app-body muted">{t("common.loading")}</p>;
  const running = started || progress !== null;
  const current: Step = running ? "practise" : step;
  const ready = !!script && !script.needsClassification && script.items.length > 0 && answerTurns(script) > 0;

  return (
    <div className="app practice">
      <header className="app-header">
        <h1>{t("practice.title")}</h1>
        <p>{t("practice.lead")}</p>
      </header>
      <div className="app-body">
        <ol className="steps" aria-label={t("practice.stepsLabel")}>
          {STEPS.map((id, i) => (
            <li key={id} aria-current={id === current ? "step" : undefined} data-done={i < STEPS.indexOf(current)}>
              {i + 1}. {t(`practice.step.${id}`)}
            </li>
          ))}
        </ol>
        {error && (
          <div className="note note-err" role="alert">
            <strong>{errorTitle(error)}</strong> <span className="mono">{error}</span>
            <p>{errorFix(error)}</p>
          </div>
        )}
        {finished && !running && (
          <p className="note" role="status">
            {t("practice.finishedBody")}
          </p>
        )}
        {running ? (
          <Running settings={settings} progress={progress} />
        ) : current === "sound" ? (
          <SoundCheck settings={settings} update={update} onNext={() => setStep("questions")} />
        ) : current === "questions" ? (
          <ChooseStep
            busy={busy === "import"}
            onImport={importFile}
            onSample={loadSample}
            finished={finished}
            onBack={() => setStep("sound")}
          />
        ) : (
          script && (
            <>
              <Preview
                script={script}
                onChange={setScript}
                busy={busy}
                onClassify={() =>
                  run("classify", async () => {
                    setScript(await backend().invoke("practice_classify", { script }));
                  })
                }
              />
              <Options settings={settings} update={update} />
              <p className="practice-next" role="status">
                {t("practice.nextStart", { seconds: seconds(settings.practice.answerPauseS * 1000) })}
              </p>
              <div className="practice-actions">
                <button
                  className="btn"
                  onClick={() => {
                    setScript(null);
                    setStep("questions");
                  }}
                  disabled={busy !== null}
                >
                  {t("practice.back")}
                </button>
                <button
                  className="btn btn-primary"
                  disabled={busy !== null || !ready}
                  onClick={() =>
                    run("start", async () => {
                      await backend().invoke("practice_start", { script });
                      setStarted(true);
                    })
                  }
                >
                  {busy === "start" ? t("practice.starting") : t("practice.start")}
                </button>
              </div>
            </>
          )
        )}
      </div>
    </div>
  );
}

/** Step 1: headphones, levels and how the questions are spoken. */
function SoundCheck({ settings, update, onNext }: { settings: Settings; update: Update; onNext: () => void }) {
  const { t } = useTranslation();
  const levels = useLevels();
  useEffect(() => {
    void backend().invoke("audio_preview", { on: true });
    return () => void backend().invoke("audio_preview", { on: false });
  }, []);
  return (
    <>
      <Section title={t("practice.soundTitle")} lead={t("practice.soundLead")}>
        <div className="note">{t("practice.headphones")}</div>
        <Field label={t("overlay.sourceMe")} hint={t("practice.micHint")}>
          <LevelBar db={levels.me} label={t("overlay.sourceMe")} />
        </Field>
        <Field label={t("overlay.sourceRemote")} hint={t("practice.systemHint")}>
          <LevelBar db={levels.remote} label={t("overlay.sourceRemote")} />
        </Field>
        <p className="muted">
          {t("practice.audioSettings")}{" "}
          <button className="btn btn-small" onClick={() => void backend().invoke("open_window", { view: "settings", section: "audio" })}>
            {t("practice.openAudio")}
          </button>
        </p>
      </Section>
      <VoiceStep settings={settings} update={update} />
      <Section title={t("practice.briefingTitle")} lead={t("practice.howBriefing")}>
        <button className="btn btn-small" onClick={() => void backend().invoke("open_window", { view: "settings", section: "hints" })}>
          {t("practice.openHints")}
        </button>
      </Section>
      <p className="practice-next" role="status">
        {t("practice.nextChoose")}
      </p>
      <div className="practice-actions">
        <button className="btn btn-primary" onClick={onNext}>
          {t("practice.toQuestions")}
        </button>
      </div>
    </>
  );
}

/** The voice that reads the questions, per language, with a sample. */
function VoiceStep({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const [voices, setVoices] = useState<Voice[] | null>(null);
  useEffect(() => {
    void backend()
      .invoke("list_voices")
      .then(setVoices)
      .catch(() => setVoices([]));
  }, []);
  const p = settings.practice;
  const setP = (patch: Partial<Settings["practice"]>) => void update((s) => ({ ...s, practice: { ...s.practice, ...patch } }));
  const langs: [("zh" | "en"), string, string][] = [
    ["en", t("practice.voiceEn"), p.voiceEn],
    ["zh", t("practice.voiceZh"), p.voiceZh],
  ];
  return (
    <Section title={t("practice.voiceTitle")} lead={t("practice.voiceLead")}>
      {voices !== null && voices.length === 0 && <p className="note note-warn">{t("practice.noVoices")}</p>}
      {langs.map(([lang, label, value]) => {
        const forLang = (voices ?? []).filter((v) => v.lang.toLowerCase().startsWith(lang));
        return (
          <Field key={lang} label={label} htmlFor={`voice-${lang}`} hint={t("practice.voiceHint")}>
            <select
              id={`voice-${lang}`}
              className="input-wide"
              value={value}
              disabled={forLang.length === 0}
              onChange={(e) => setP(lang === "zh" ? { voiceZh: e.target.value } : { voiceEn: e.target.value })}
            >
              <option value="">{t("practice.voiceAuto")}</option>
              {forLang.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.name}
                </option>
              ))}
            </select>{" "}
            <button
              className="btn btn-small"
              disabled={forLang.length === 0}
              onClick={() => void backend().invoke("preview_voice", { voice: value, lang })}
            >
              {t("practice.tryVoice")}
            </button>
          </Field>
        );
      })}
      <Field label={t("practice.speechRate")} htmlFor="rate">
        <input id="rate" type="range" min={0.6} max={1.6} step={0.1} value={p.speechRate} onChange={(e) => setP({ speechRate: Number(e.target.value) })} />{" "}
        <span className="mono">{p.speechRate.toFixed(1)}×</span>
      </Field>
    </Section>
  );
}

/** Sample interviews built into the app; names match `practice::SAMPLES`. */
export const SAMPLES = [
  { key: "sweQuestions", file: "questions-software-engineer.csv" },
  { key: "sweInterview", file: "swe-interview.srt" },
  { key: "securityQuestions", file: "questions-security-ai.csv" },
  { key: "securityInterview", file: "security-ai-interview.srt" },
  { key: "networkQuestions", file: "questions-network.csv" },
  { key: "sdnInterview", file: "sdn-interview.srt" },
] as const;

function ChooseStep({
  busy,
  onImport,
  onSample,
  finished,
  onBack,
}: {
  busy: boolean;
  onImport: () => void;
  onSample: (file: string) => void;
  finished: boolean;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const [reports, setReports] = useState<PracticeReportSummary[]>([]);
  useEffect(() => {
    void backend()
      .invoke("practice_reports")
      .then(setReports)
      .catch(() => setReports([]));
  }, [finished]);
  return (
    <>
      <Section title={t("practice.samplesTitle")} lead={t("practice.samplesLead")}>
        <ul className="practice-samples">
          {SAMPLES.map((s) => (
            <li key={s.file}>
              <span>
                <strong>{t(`practice.sample.${s.key}.title`)}</strong>
                <span className="muted">{t(`practice.sample.${s.key}.body`)}</span>
              </span>
              <button className="btn btn-small" onClick={() => onSample(s.file)} disabled={busy}>
                {t("practice.useSample")}
              </button>
            </li>
          ))}
        </ul>
      </Section>
      <Section title={t("practice.importTitle")} lead={t("practice.importHint")}>
        <button className="btn" onClick={onImport} disabled={busy}>
          {t("practice.import")}
        </button>
      </Section>
      <Section title={t("practice.historyTitle")}>
        {reports.length === 0 ? (
          <p className="muted">{t("practice.historyEmpty")}</p>
        ) : (
          <ul className="practice-history">
            {reports.map((r) => (
              <li key={r.id}>
                <span>
                  <strong>{r.title}</strong>{" "}
                  <span className="muted">
                    {new Date(r.startedAt).toLocaleString()} · {t("practice.countQuestions", { count: r.questions })}
                  </span>
                </span>
                <button className="btn btn-small" onClick={() => void backend().invoke("open_window", { view: "review", section: r.id })}>
                  {t("practice.openReview")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>
      <p className="practice-next" role="status">
        {t("practice.nextCheck")}
      </p>
      <div className="practice-actions">
        <button className="btn" onClick={onBack}>
          {t("practice.toSound")}
        </button>
      </div>
    </>
  );
}

function Preview({
  script,
  onChange,
  busy,
  onClassify,
}: {
  script: PracticeScript;
  onChange: (s: PracticeScript) => void;
  busy: string | null;
  onClassify: () => void;
}) {
  const { t } = useTranslation();
  const [keep, setKeep] = useState(String(script.items.length));
  const subtitles = script.kind === "subtitles";
  const turns = answerTurns(script);
  return (
    <Section
      title={t("practice.previewTitle")}
      lead={
        <>
          <strong>{script.title}</strong> ·{" "}
          {subtitles ? t("practice.countTurns", { count: turns }) : t("practice.countQuestions", { count: script.items.length })}
        </>
      }
    >
      {subtitles && script.needsClassification && (
        <div className="note note-warn">
          <strong>{t("practice.classifyTitle")}</strong>
          <p>{t("practice.classifyBody")}</p>
          <button className="btn btn-small" onClick={onClassify} disabled={busy !== null}>
            {busy === "classify" ? t("practice.classifying") : t("practice.classify")}
          </button>
        </div>
      )}
      {subtitles && script.speakers.length > 0 && (
        <Field label={t("practice.meSpeaker")} hint={t("practice.meSpeakerHint")} htmlFor="me-speaker">
          <select
            id="me-speaker"
            value={script.meSpeaker ?? ""}
            onChange={(e) => onChange(setMeSpeaker(script, e.target.value || null))}
          >
            <option value="">—</option>
            {script.speakers.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </Field>
      )}
      {!subtitles && (
        <div className="practice-tools">
          <button className="btn btn-small" onClick={() => onChange(shuffleItems(script))}>
            {t("practice.shuffle")}
          </button>
          <label className="practice-keep">
            {t("practice.keepFirst")}{" "}
            <input type="number" min={1} max={script.items.length} value={keep} onChange={(e) => setKeep(e.target.value)} />
          </label>
          <button className="btn btn-small" onClick={() => onChange(keepFirst(script, Number(keep)))}>
            {t("practice.apply")}
          </button>
        </div>
      )}
      <div className="table-wrap">
        <table className="data practice-table">
          <thead>
            <tr>
              <th scope="col">{t("practice.colNo")}</th>
              {subtitles && <th scope="col">{t("practice.colSpeaker")}</th>}
              <th scope="col">{t("practice.colText")}</th>
              {!subtitles && <th scope="col">{t("practice.colGroup")}</th>}
              <th scope="col">{t("practice.colActions")}</th>
            </tr>
          </thead>
          <tbody>
            {script.items.map((it, i) => (
              <tr key={`${it.id}-${i}`} data-me={it.isMe}>
                <td className="mono">{it.id}</td>
                {subtitles && (
                  <td>
                    {it.speaker ?? (it.isMe ? t("practice.me") : t("practice.question"))}
                    {it.isMe && it.speaker && <span className="badge badge-ok">{t("practice.me")}</span>}
                  </td>
                )}
                <td className="text">
                  <input
                    aria-label={`${t("practice.colText")} ${it.id}`}
                    value={it.text}
                    onChange={(e) => onChange(editItem(script, i, e.target.value))}
                  />
                  {it.reference && <span className="muted ref">{it.reference}</span>}
                </td>
                {!subtitles && <td>{it.category ?? ""}</td>}
                <td className="actions">
                  <button className="btn btn-small" aria-label={t("practice.moveUp")} title={t("practice.moveUp")} disabled={i === 0} onClick={() => onChange(moveItem(script, i, -1))}>
                    ↑
                  </button>
                  <button
                    className="btn btn-small"
                    aria-label={t("practice.moveDown")}
                    title={t("practice.moveDown")}
                    disabled={i === script.items.length - 1}
                    onClick={() => onChange(moveItem(script, i, 1))}
                  >
                    ↓
                  </button>
                  <button className="btn btn-small" aria-label={t("practice.delete")} title={t("practice.delete")} onClick={() => onChange(removeItem(script, i))}>
                    ✕
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  );
}

function Options({ settings, update }: { settings: Settings; update: Update }) {
  const { t } = useTranslation();
  const p = settings.practice;
  const setP = (patch: Partial<Settings["practice"]>) => void update((s) => ({ ...s, practice: { ...s.practice, ...patch } }));
  const levels: [HintLevel, string][] = [
    ["full", t("practice.levelFull")],
    ["headsOnly", t("practice.levelHeads")],
    ["delayed", t("practice.levelDelayed")],
    ["off", t("practice.levelOff")],
  ];
  return (
    <Section title={t("practice.optionsTitle")}>
      <Switch
        checked={p.playback === "realPipeline"}
        onChange={(on) => setP({ playback: on ? "realPipeline" : "simulatedLatency" })}
        label={t("practice.sttQuestions")}
        description={p.playback === "realPipeline" ? t("practice.sttQuestionsOn") : t("practice.sttQuestionsOff")}
      />
      {p.playback === "simulatedLatency" && <p className="note">{t("practice.simulatedNote")}</p>}
      <Switch
        checked={p.recognizeMe}
        onChange={(v) => setP({ recognizeMe: v })}
        label={t("practice.recognizeMe")}
        description={t("practice.recognizeMeHint")}
      />
      <Field label={t("practice.level")} hint={t("practice.levelHint")}>
        <div className="btn-group" role="group" aria-label={t("practice.level")}>
          {levels.map(([value, label]) => (
            <button key={value} className="btn btn-small" aria-pressed={p.hintLevel === value} onClick={() => setP({ hintLevel: value })}>
              {label}
            </button>
          ))}
        </div>
        {p.hintLevel === "delayed" && (
          <label className="practice-inline">
            {t("practice.delay")}{" "}
            <input type="number" min={1} max={60} value={p.hintDelayS} onChange={(e) => setP({ hintDelayS: Number(e.target.value) || 1 })} />
          </label>
        )}
      </Field>
      <Field label={t("practice.answerPause")} htmlFor="answer-pause">
        <input id="answer-pause" type="number" min={1} max={15} step={0.5} value={p.answerPauseS} onChange={(e) => setP({ answerPauseS: Number(e.target.value) || 3 })} />
      </Field>
      <Field label={t("practice.nudge")} htmlFor="nudge">
        <input id="nudge" type="number" min={5} max={120} value={p.nudgeAfterS} onChange={(e) => setP({ nudgeAfterS: Number(e.target.value) || 20 })} />
      </Field>
      <Switch
        checked={p.followUpProbability > 0}
        onChange={(on) => setP({ followUpProbability: on ? 0.3 : 0 })}
        label={t("practice.followUps")}
        description={t("practice.followUpsHint")}
      />
      {p.followUpProbability > 0 && (
        <Field label={t("practice.followUpChance")} htmlFor="follow-chance">
          <input
            id="follow-chance"
            type="range"
            min={0.1}
            max={1}
            step={0.1}
            value={p.followUpProbability}
            onChange={(e) => setP({ followUpProbability: Number(e.target.value) })}
          />{" "}
          <span className="mono">{Math.round(p.followUpProbability * 100)}%</span>
        </Field>
      )}
      <Switch
        checked={settings.audio.stopTtsOnSpeech}
        onChange={(v) => void update((s) => ({ ...s, audio: { ...s.audio, stopTtsOnSpeech: v } }))}
        label={t("practice.stopOnSpeech")}
      />
      <Field label={t("practice.fillersZh")} htmlFor="fillers-zh" hint={t("practice.fillersHint")}>
        <WordList id="fillers-zh" words={p.fillersZh} onChange={(fillersZh) => setP({ fillersZh })} />
      </Field>
      <Field label={t("practice.fillersEn")} htmlFor="fillers-en">
        <WordList id="fillers-en" words={p.fillersEn} onChange={(fillersEn) => setP({ fillersEn })} />
      </Field>
    </Section>
  );
}

/** Words separated by commas (either width) or 、, edited as one line. */
export function splitWords(text: string): string[] {
  return text
    .split(/[,，、]/)
    .map((w) => w.trim())
    .filter(Boolean);
}

function WordList({ id, words, onChange }: { id: string; words: string[]; onChange: (w: string[]) => void }) {
  // Keep what's typed (including a trailing comma) until focus leaves.
  const [text, setText] = useState(words.join(", "));
  useEffect(() => setText(words.join(", ")), [words]);
  return (
    <input
      id={id}
      className="input-wide"
      type="text"
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => onChange(splitWords(text))}
    />
  );
}

function Running({ settings, progress }: { settings: Settings; progress: PracticeProgress | null }) {
  const { t } = useTranslation();
  const paused = progress?.phase === "paused";
  const control = (c: "pause" | "resume" | "skip" | "stop" | "answered" | "repeat") =>
    void backend().invoke("practice_control", { control: c });
  const next = progress ? t(`practice.next.${progress.phase}`, { seconds: seconds(settings.practice.answerPauseS * 1000) }) : "";
  return (
    <Section title={t("practice.runningTitle")} lead={t("practice.runningBody", { seconds: seconds(settings.practice.answerPauseS * 1000) })}>
      {progress && (
        <div className="practice-progress" aria-live="polite">
          <strong>{t("practice.progress", { index: progress.index, total: progress.total })}</strong>
          <span>{t(`practice.phase.${progress.phase}`)}</span>
          {progress.question && <p className="muted">{progress.question}</p>}
          {progress.nudge && <p className="note">{t("practice.phase.nudge")}</p>}
          {next && <p className="practice-next">{next}</p>}
        </div>
      )}
      <div className="practice-actions">
        <button className="btn" onClick={() => control("repeat")}>
          {t("practice.repeat")}
        </button>
        <button className="btn" onClick={() => control(paused ? "resume" : "pause")}>
          {paused ? t("practice.resume") : t("practice.pause")}
        </button>
        <button className="btn" onClick={() => control("answered")}>
          {t("practice.answered")}
        </button>
        <button className="btn" onClick={() => control("skip")}>
          {t("practice.skip")}
        </button>
        <button className="btn" onClick={() => control("stop")}>
          {t("practice.stop")}
        </button>
      </div>
    </Section>
  );
}
