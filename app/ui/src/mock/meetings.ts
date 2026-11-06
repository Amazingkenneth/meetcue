// Saved meetings for the browser preview: a sample one, plus whatever the
// preview session produced when it is stopped.

import type { MeetingRecord } from "../bindings/MeetingRecord";
import type { MeetingSummary } from "../bindings/MeetingSummary";
import type { SessionView } from "../state/session";

export function meetingSummary(m: MeetingRecord): MeetingSummary {
  const points = m.questions.flatMap((q) => q.points);
  const first = m.questions[0];
  return {
    id: m.id,
    startedAt: m.startedAt,
    durationMs: m.durationMs,
    questions: m.questions.length,
    pointsDone: points.filter((p) => p.done).length,
    pointsTotal: points.length,
    costMicros: m.costMicros,
    firstQuestion: first ? first.short || first.text : "",
  };
}

function stamp(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

/** A meeting from what the preview panel showed, like the shell saves it. */
export function meetingFromView(view: SessionView, durationMs: number, keepTranscript: boolean): MeetingRecord | null {
  const start = view.startedAt ?? 0;
  const questions = view.order
    .map((id) => view.questions[id])
    .filter(Boolean)
    .map((q) => ({
      atMs: Math.max(0, q.tMs - start),
      text: q.text,
      short: q.short,
      ...(q.speaker ? { speaker: q.speaker } : {}),
      status: q.status,
      points: q.points.map((p) => ({ head: p.head, rest: p.rest, done: p.status === "done" })),
      answer: q.answer.trim(),
    }));
  const transcript = keepTranscript ? view.transcript.map((l) => ({ ...l, t0Ms: Math.max(0, l.t0Ms - start), t1Ms: Math.max(0, l.t1Ms - start) })) : [];
  if (!questions.length && !transcript.length) return null;
  const started = new Date(Date.now() - durationMs);
  return {
    id: stamp(started),
    startedAt: started.toISOString(),
    durationMs,
    costMicros: view.sessionMicros,
    questions,
    transcript,
  };
}

export function sampleMeeting(): MeetingRecord {
  const started = new Date(Date.now() - 26 * 3600_000);
  return {
    id: stamp(started),
    startedAt: started.toISOString(),
    durationMs: 31 * 60_000,
    costMicros: 118_000,
    questions: [
      {
        atMs: 95_000,
        text: "How do you keep the network highly available?",
        short: "Network high availability",
        status: "answered",
        points: [
          { head: "Active-active data centres", rest: "Two sites share the traffic, so either can take all of it.", done: true },
          { head: "Redundant devices", rest: "Stacked core switches with dual power supplies.", done: true },
          { head: "Fast failover", rest: "BGP multipath with BFD detects a failed link in under a second.", done: false },
        ],
        answer: "We run **two active-active data centres** that share traffic, with **stacked core switches** and dual power. Links are bundled with LACP, and **BGP multipath with BFD** moves traffic within a second when something fails.",
      },
      {
        atMs: 610_000,
        text: "What would you change if traffic doubled next year?",
        short: "Plan for double traffic",
        status: "answered",
        points: [
          { head: "Measure first", rest: "Look at peak utilisation per link and per site before buying anything.", done: true },
          { head: "Scale out", rest: "Add spine capacity and a third uplink rather than bigger single boxes.", done: false },
        ],
        answer: "I'd start from **peak utilisation per link**, then **scale out** the spine layer and add uplinks, keeping 40% headroom.",
      },
    ],
    transcript: [],
  };
}
