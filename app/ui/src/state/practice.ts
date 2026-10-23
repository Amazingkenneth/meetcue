import { useState } from "react";
import type { PracticeItem } from "../bindings/PracticeItem";
import type { PracticeScript } from "../bindings/PracticeScript";
import { useAppEvents } from "./hooks";

export interface PracticeProgress {
  index: number;
  total: number;
  phase: string;
  question: string;
  /** The user hasn't started answering for a while. */
  nudge: boolean;
}

/** Follows the practice run reported by the backend; `null` when none is running. */
export function usePracticeProgress(): PracticeProgress | null {
  const [p, setP] = useState<PracticeProgress | null>(null);
  useAppEvents((e) => {
    if (e.type === "practiceProgress") {
      if (e.phase === "nudge") {
        setP((prev) => (prev ? { ...prev, nudge: true } : prev));
      } else if (e.phase === "done") {
        setP(null);
      } else {
        setP({ index: e.index, total: e.total, phase: e.phase, question: e.question, nudge: false });
      }
    } else if (e.type === "practiceFinished") {
      setP(null);
    }
  });
  return p;
}

/** Number of answer turns: questions, or the user's own subtitle turns. */
export function answerTurns(script: PracticeScript): number {
  if (script.kind === "questions") return script.items.length;
  let turns = 0;
  let prevMe = false;
  for (const it of script.items) {
    if (it.isMe && !prevMe) turns++;
    prevMe = it.isMe;
  }
  return turns;
}

function renumber(items: PracticeItem[]): PracticeItem[] {
  return items.map((it, i) => ({ ...it, id: i + 1 }));
}

export function moveItem(script: PracticeScript, index: number, delta: number): PracticeScript {
  const to = index + delta;
  if (to < 0 || to >= script.items.length) return script;
  const items = [...script.items];
  const [it] = items.splice(index, 1);
  items.splice(to, 0, it);
  return { ...script, items: renumber(items) };
}

export function removeItem(script: PracticeScript, index: number): PracticeScript {
  return { ...script, items: renumber(script.items.filter((_, i) => i !== index)) };
}

export function editItem(script: PracticeScript, index: number, text: string): PracticeScript {
  return { ...script, items: script.items.map((it, i) => (i === index ? { ...it, text } : it)) };
}

/** Fisher–Yates shuffle with an injectable random source for tests. */
export function shuffleItems(script: PracticeScript, random: () => number = Math.random): PracticeScript {
  const items = [...script.items];
  for (let i = items.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [items[i], items[j]] = [items[j], items[i]];
  }
  return { ...script, items: renumber(items) };
}

export function keepFirst(script: PracticeScript, n: number): PracticeScript {
  if (!Number.isFinite(n) || n < 1) return script;
  return { ...script, items: renumber(script.items.slice(0, Math.floor(n))) };
}

export function setMeSpeaker(script: PracticeScript, speaker: string | null): PracticeScript {
  return {
    ...script,
    meSpeaker: speaker,
    items: script.items.map((it) => ({ ...it, isMe: speaker !== null && it.speaker === speaker })),
  };
}
