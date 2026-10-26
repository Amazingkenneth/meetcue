import { describe, expect, it } from "vitest";
import type { MeetCueEvent } from "../bindings/MeetCueEvent";
import demo from "../mock/sdn-demo.json";
import type { TimedEvent } from "../bindings/TimedEvent";
import { coverageCount, currentId, displayOrder, emptySession, neighbours, nextPoint, reduce } from "./session";

const q = (id: number, version = 1, status: "active" | "pending" = "active"): MeetCueEvent => ({
  type: "questionNew",
  id,
  version,
  text: `question ${id}`,
  short: `Q${id}`,
  tags: [],
  subQuestions: [],
  status,
  tMs: 1000,
  context: [],
});

const point = (id: number, n: number, version = 1): MeetCueEvent => ({
  type: "questionPoint",
  id,
  version,
  n,
  head: `h${n}`,
  rest: "r",
  status: n === 1 ? "next" : "todo",
});

function run(events: MeetCueEvent[]) {
  return events.reduce(reduce, emptySession());
}

describe("session reducer", () => {
  it("builds questions with points and coverage", () => {
    const s = run([
      { type: "sessionState", state: "running", tMs: 500 },
      q(1),
      point(1, 1),
      point(1, 2),
      { type: "coverage", id: 1, statuses: ["done", "next"] },
    ]);
    expect(s.startedAt).toBe(500);
    expect(s.questions[1].points.map((p) => p.status)).toEqual(["done", "next"]);
    expect(coverageCount(s.questions[1])).toEqual({ done: 1, total: 2 });
    expect(nextPoint(s.questions[1])?.head).toBe("h2");
  });

  it("buffers points that arrive before their question", () => {
    const s = run([point(1, 1), { type: "answerDelta", id: 1, version: 1, text: "Hi" }, q(1)]);
    expect(s.questions[1].points).toHaveLength(1);
    expect(s.questions[1].answer).toBe("Hi");
    expect(s.pending[1]).toBeUndefined();
  });

  it("drops stale versions and resets on a new version", () => {
    let s = run([q(1), point(1, 1), q(1, 2)]);
    expect(s.questions[1].points).toHaveLength(0);
    s = reduce(s, point(1, 5, 1));
    expect(s.questions[1].points).toHaveLength(0);
    s = reduce(s, point(1, 1, 2));
    expect(s.questions[1].points).toHaveLength(1);
    // Same version again updates the text but keeps the points.
    s = reduce(s, { ...(q(1, 2) as Extract<MeetCueEvent, { type: "questionNew" }>), text: "updated" });
    expect(s.questions[1].text).toBe("updated");
    expect(s.questions[1].points).toHaveLength(1);
  });

  it("tracks queue, focus and neighbours", () => {
    let s = run([q(1), q(2, 1, "pending"), q(3, 1, "pending")]);
    s = reduce(s, {
      type: "queueChanged",
      questions: [
        { id: 1, short: "Q1", status: "answered" },
        { id: 2, short: "Q2", status: "active" },
        { id: 3, short: "Q3", status: "pending" },
      ],
      active: 2,
      focus: 2,
    });
    expect(currentId(s)).toBe(2);
    const n = neighbours(s);
    expect([n.prev?.id, n.current?.id, n.next?.id]).toEqual([1, 2, 3]);
    expect(s.questions[1].status).toBe("answered");
  });

  it("lists the current question first, then waiting ones, then earlier ones newest first", () => {
    let s = run([q(1), q(2), q(3), q(4, 1, "pending")]);
    s = reduce(s, {
      type: "queueChanged",
      questions: [
        { id: 1, short: "Q1", status: "answered" },
        { id: 2, short: "Q2", status: "answered" },
        { id: 3, short: "Q3", status: "active" },
        { id: 4, short: "Q4", status: "pending" },
      ],
      active: 3,
      focus: null,
    });
    expect(displayOrder(s)).toEqual([3, 4, 2, 1]);
    expect(displayOrder(emptySession())).toEqual([]);
  });

  it("keeps partial and final transcript lines apart", () => {
    let s = run([{ type: "transcript", channel: "me", utterance: 1, isFinal: false, text: "我们", t0Ms: 0, t1Ms: 100 }]);
    expect(s.partial.me?.text).toBe("我们");
    s = reduce(s, { type: "transcript", channel: "me", utterance: 1, isFinal: true, text: "我们好", t0Ms: 0, t1Ms: 200 });
    expect(s.partial.me).toBeNull();
    expect(s.transcript).toHaveLength(1);
    // An echo is cleared with an empty final and never shown.
    s = reduce(s, { type: "transcript", channel: "me", utterance: 2, isFinal: true, text: "", t0Ms: 0, t1Ms: 200 });
    expect(s.transcript).toHaveLength(1);
  });

  it("clears connection errors once every channel recognises again", () => {
    let s = run([
      { type: "sessionState", state: "running", tMs: 0 },
      { type: "recognition", channel: "me", connected: true },
      { type: "recognition", channel: "remote", connected: true },
      { type: "error", code: "MC-ASR-002", detail: "stt-rt.soniox.com: connection timed out" },
      { type: "recognition", channel: "remote", connected: false },
      { type: "error", code: "MC-ASR-001", detail: "HTTP 401" },
    ]);
    expect(s.recognition).toEqual({ me: true, remote: false });
    expect(s.errors.map((e) => e.code)).toEqual(["MC-ASR-002", "MC-ASR-001"]);
    // The microphone reconnecting alone doesn't clear the system sound's outage.
    s = reduce(s, { type: "recognition", channel: "me", connected: true });
    expect(s.errors).toHaveLength(2);
    s = reduce(s, { type: "recognition", channel: "remote", connected: true });
    expect(s.errors.map((e) => e.code)).toEqual(["MC-ASR-001"]);
  });

  it("replays the recorded demo into a consistent state", () => {
    const s = (demo as TimedEvent[]).map((t) => t.event).reduce(reduce, emptySession());
    expect(s.order).toEqual([1, 2, 3]);
    expect(Object.values(s.questions).every((x) => x.status === "answered")).toBe(true);
    expect(Object.values(s.questions).every((x) => x.points.length >= 3 && x.answerDone)).toBe(true);
    expect(s.latencies).toHaveLength(3);
    expect(s.sessionMicros).toBeGreaterThan(0);
  });
});
