import { useEffect, useState } from "react";
import { backend } from "../backend";
import type { PracticeItem } from "../bindings/PracticeItem";
import type { PracticeScript } from "../bindings/PracticeScript";
import { useAppEvents } from "./hooks";

export interface PracticeProgress {
  index: number;
  total: number;
  /** The script's own turn (1-based), matching `usePracticeOutline`. */
  turn: number;
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
        setP({ index: e.index, total: e.total, turn: e.turn, phase: e.phase, question: e.question, nudge: false });
      }
    } else if (e.type === "practiceFinished") {
      setP(null);
    }
  });
  return p;
}

/** The question of each answer turn of the running practice, to jump between them. */
export function usePracticeOutline(running: boolean): string[] {
  const [outline, setOutline] = useState<string[]>([]);
  useEffect(() => {
    if (!running) {
      setOutline([]);
      return;
    }
    let live = true;
    void backend()
      .invoke("practice_outline")
      .then((o) => live && setOutline(o))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [running]);
  return outline;
}

/** Goes to answer turn `turn` (1-based) of the running practice. */
export function jumpTo(turn: number) {
  void backend().invoke("practice_jump", { turn });
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

/** Item positions picked in the preview, for batch changes. */
export type Selection = ReadonlySet<number>;

export function removeSelected(script: PracticeScript, sel: Selection): PracticeScript {
  return { ...script, items: renumber(script.items.filter((_, i) => !sel.has(i))) };
}

export function keepSelected(script: PracticeScript, sel: Selection): PracticeScript {
  if (sel.size === 0) return script;
  return { ...script, items: renumber(script.items.filter((_, i) => sel.has(i))) };
}

/** Moves the selected items, in their current order, to the top or the bottom. */
export function moveSelected(script: PracticeScript, sel: Selection, to: "top" | "bottom"): PracticeScript {
  const picked = script.items.filter((_, i) => sel.has(i));
  const rest = script.items.filter((_, i) => !sel.has(i));
  return { ...script, items: renumber(to === "top" ? [...picked, ...rest] : [...rest, ...picked]) };
}

export function setGroup(script: PracticeScript, sel: Selection, group: string): PracticeScript {
  const category = group.trim() || null;
  return { ...script, items: script.items.map((it, i) => (sel.has(i) ? { ...it, category } : it)) };
}

/** Appends a question typed by the user. */
export function addQuestion(script: PracticeScript, text: string): PracticeScript {
  const t = text.trim();
  if (!t) return script;
  const item: PracticeItem = {
    id: script.items.length + 1,
    text: t,
    speaker: null,
    startMs: null,
    endMs: null,
    isMe: false,
    isQuestion: true,
    reference: null,
    category: null,
    followups: [],
  };
  return { ...script, items: [...script.items, item] };
}

/** The groups in the list, in order of first appearance. */
export function groups(script: PracticeScript): string[] {
  return [...new Set(script.items.map((it) => it.category).filter((c): c is string => !!c))];
}

/** Positions from `from` to `to`, inclusive, either way round. */
export function range(from: number, to: number): number[] {
  const [a, b] = from <= to ? [from, to] : [to, from];
  return Array.from({ length: b - a + 1 }, (_, i) => a + i);
}
