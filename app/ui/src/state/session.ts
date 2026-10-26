// Pure reducer that turns pipeline events into what the overlay shows.
//
// Events can arrive out of order (a hint point before its question, or a
// point of an older version after a newer question). Points for unknown
// questions are buffered per question id and applied when the question
// arrives; anything for an older version is dropped.

import type { Channel } from "../bindings/Channel";
import type { MeetCueEvent } from "../bindings/MeetCueEvent";
import type { PointStatus } from "../bindings/PointStatus";
import type { QuestionStatus } from "../bindings/QuestionStatus";
import type { SessionState } from "../bindings/SessionState";
import type { TranscriptLine } from "../bindings/TranscriptLine";
import type { LatencyBreakdown } from "./latency";

export interface PointView {
  n: number;
  head: string;
  rest: string;
  status: PointStatus;
}

export interface QuestionView {
  id: number;
  version: number;
  text: string;
  short: string;
  tags: string[];
  subQuestions: string[];
  status: QuestionStatus;
  tMs: number;
  context: TranscriptLine[];
  points: PointView[];
  answer: string;
  answerDone: boolean;
  failedCode: string | null;
  latencyMs: number | null;
}

export interface Partial {
  utterance: number;
  text: string;
}

export interface SessionErrorView {
  code: string;
  detail: string;
  at: number;
}

export interface SessionView {
  state: SessionState;
  startedAt: number | null;
  questions: Record<number, QuestionView>;
  order: number[];
  active: number | null;
  focus: number | null;
  transcript: TranscriptLine[];
  partial: Record<Channel, Partial | null>;
  levels: Record<Channel, number>;
  /// Whether each channel's recognition stream is open; null before it reports.
  recognition: Record<Channel, boolean | null>;
  latencies: number[];
  /// How the last hint's time split between recognition, detection and hints.
  lastLatency: LatencyBreakdown | null;
  latencyWarning: number | null;
  sessionMicros: number;
  errors: SessionErrorView[];
  budget: { scope: string; level: string } | null;
  pending: Record<number, MeetCueEvent[]>;
  /// Monotonic counter of received events (useful for effects).
  seq: number;
}

const TRANSCRIPT_LIMIT = 400;
const ERROR_LIMIT = 20;
/// Errors about reaching speech recognition, which stop applying once every
/// channel is connected again.
const CONNECTION_ERRORS = new Set(["MC-ASR-002", "MC-ASR-003"]);

export function emptySession(): SessionView {
  return {
    state: "idle",
    startedAt: null,
    questions: {},
    order: [],
    active: null,
    focus: null,
    transcript: [],
    partial: { me: null, remote: null },
    levels: { me: -100, remote: -100 },
    recognition: { me: null, remote: null },
    latencies: [],
    lastLatency: null,
    latencyWarning: null,
    sessionMicros: 0,
    errors: [],
    budget: null,
    pending: {},
    seq: 0,
  };
}

function withQuestion(s: SessionView, id: number, f: (q: QuestionView) => QuestionView): SessionView {
  const q = s.questions[id];
  if (!q) return s;
  return { ...s, questions: { ...s.questions, [id]: f(q) } };
}

function buffer(s: SessionView, id: number, e: MeetCueEvent): SessionView {
  const list = s.pending[id] ?? [];
  return { ...s, pending: { ...s.pending, [id]: [...list, e] } };
}

export function reduce(prev: SessionView, e: MeetCueEvent): SessionView {
  const s = { ...prev, seq: prev.seq + 1 };
  switch (e.type) {
    case "sessionState": {
      if (e.state === "running" && prev.state !== "paused") {
        // A new session starts from a clean slate.
        return { ...emptySession(), state: "running", startedAt: e.tMs, seq: s.seq };
      }
      return { ...s, state: e.state };
    }
    case "transcript": {
      if (!e.isFinal) {
        return { ...s, partial: { ...s.partial, [e.channel]: { utterance: e.utterance, text: e.text } } };
      }
      const partial = { ...s.partial, [e.channel]: null };
      if (!e.text) return { ...s, partial };
      const line: TranscriptLine = { channel: e.channel, text: e.text, t0Ms: e.t0Ms, t1Ms: e.t1Ms };
      const transcript = [...s.transcript, line].slice(-TRANSCRIPT_LIMIT);
      return { ...s, partial, transcript };
    }
    case "questionNew": {
      const existing = s.questions[e.id];
      if (existing && e.version < existing.version) return s;
      const reset = !existing || e.version > existing.version;
      const q: QuestionView = {
        id: e.id,
        version: e.version,
        text: e.text,
        short: e.short,
        tags: e.tags,
        subQuestions: e.subQuestions,
        status: existing?.status ?? e.status,
        tMs: e.tMs,
        context: e.context,
        points: reset ? [] : existing.points,
        answer: reset ? "" : existing.answer,
        answerDone: reset ? false : existing.answerDone,
        failedCode: reset ? null : existing.failedCode,
        latencyMs: existing?.latencyMs ?? null,
      };
      let next: SessionView = {
        ...s,
        questions: { ...s.questions, [e.id]: q },
        order: existing ? s.order : [...s.order, e.id],
      };
      // Apply buffered events that belong to this version.
      const waiting = next.pending[e.id];
      if (waiting) {
        const rest = { ...next.pending };
        delete rest[e.id];
        next = { ...next, pending: rest };
        for (const w of waiting) next = reduce(next, w);
      }
      return next;
    }
    case "questionRemoved": {
      const questions = { ...s.questions };
      delete questions[e.id];
      return { ...s, questions, order: s.order.filter((id) => id !== e.id) };
    }
    case "questionPoint": {
      const q = s.questions[e.id];
      if (!q || e.version > q.version) return buffer(s, e.id, e);
      if (e.version < q.version) return s;
      const point: PointView = { n: e.n, head: e.head, rest: e.rest, status: e.status };
      return withQuestion(s, e.id, (q) => ({ ...q, points: [...q.points, point] }));
    }
    case "answerDelta": {
      const q = s.questions[e.id];
      if (!q || e.version > q.version) return buffer(s, e.id, e);
      if (e.version < q.version) return s;
      return withQuestion(s, e.id, (q) => ({ ...q, answer: q.answer + e.text }));
    }
    case "answerDone": {
      const q = s.questions[e.id];
      if (!q || e.version > q.version) return buffer(s, e.id, e);
      if (e.version < q.version) return s;
      return withQuestion(s, e.id, (q) => ({ ...q, answerDone: true }));
    }
    case "hintsFailed": {
      const q = s.questions[e.id];
      if (!q || e.version > q.version) return buffer(s, e.id, e);
      if (e.version < q.version) return s;
      return withQuestion(s, e.id, (q) => ({ ...q, failedCode: e.code }));
    }
    case "coverage": {
      if (!s.questions[e.id]) return buffer(s, e.id, e);
      return withQuestion(s, e.id, (q) => ({
        ...q,
        points: q.points.map((p, i) => ({ ...p, status: e.statuses[i] ?? p.status })),
      }));
    }
    case "questionStatus":
      return withQuestion(s, e.id, (q) => ({ ...q, status: e.status }));
    case "queueChanged": {
      let next: SessionView = { ...s, active: e.active, focus: e.focus };
      for (const b of e.questions) {
        next = withQuestion(next, b.id, (q) => ({ ...q, status: b.status, short: b.short || q.short }));
      }
      return next;
    }
    case "latencySample": {
      const next = withQuestion(s, e.id, (q) => ({ ...q, latencyMs: e.totalMs }));
      const lastLatency = { asrMs: e.asrMs, detectMs: e.detectMs, generateMs: e.generateMs, totalMs: e.totalMs, early: e.early };
      return { ...next, latencies: [...s.latencies, e.totalMs].slice(-20), lastLatency };
    }
    case "latencyWarning":
      return { ...s, latencyWarning: e.medianMs };
    case "usage":
      return { ...s, sessionMicros: e.sessionMicros };
    case "budget":
      return { ...s, budget: { scope: e.scope, level: e.level } };
    case "levels":
      return { ...s, levels: { me: e.me, remote: e.remote } };
    case "recognition": {
      const recognition = { ...s.recognition, [e.channel]: e.connected };
      const recovered = recognition.me !== false && recognition.remote !== false;
      const errors = recovered ? s.errors.filter((x) => !CONNECTION_ERRORS.has(x.code)) : s.errors;
      return { ...s, recognition, errors };
    }
    case "error": {
      const err = { code: e.code, detail: e.detail, at: Date.now() };
      return { ...s, errors: [...s.errors, err].slice(-ERROR_LIMIT) };
    }
  }
}

/** The question shown as current: user focus, else the active question, else the latest. */
export function currentId(s: SessionView): number | null {
  return s.focus ?? s.active ?? (s.order.length ? s.order[s.order.length - 1] : null);
}

export function neighbours(s: SessionView): { prev: QuestionView | null; current: QuestionView | null; next: QuestionView | null } {
  const id = currentId(s);
  const idx = id === null ? -1 : s.order.indexOf(id);
  const at = (i: number) => (i >= 0 && i < s.order.length ? s.questions[s.order[i]] ?? null : null);
  return { prev: at(idx - 1), current: at(idx), next: at(idx + 1) };
}

export function coverageCount(q: QuestionView): { done: number; total: number } {
  return { done: q.points.filter((p) => p.status === "done").length, total: q.points.length };
}

export function nextPoint(q: QuestionView): PointView | null {
  return q.points.find((p) => p.status === "next") ?? q.points.find((p) => p.status !== "done") ?? null;
}

export function medianLatency(s: SessionView): number | null {
  if (!s.latencies.length) return null;
  const v = [...s.latencies].sort((a, b) => a - b);
  const mid = Math.floor(v.length / 2);
  return v.length % 2 ? v[mid] : Math.round((v[mid - 1] + v[mid]) / 2);
}

/** Final transcript lines around a question for the context disclosure. */
export function contextAround(s: SessionView, q: QuestionView, before = 3, after = 3): TranscriptLine[] {
  const idx = s.transcript.findIndex((l) => l.channel === "remote" && l.t1Ms >= q.tMs - 50);
  if (idx < 0) return q.context;
  return s.transcript.slice(Math.max(0, idx - before), idx + after + 1);
}
