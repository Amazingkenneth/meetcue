import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { setBackend } from "../backend";
import type { PracticeScript } from "../bindings/PracticeScript";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { mockPracticeScript } from "../mock/practice";
import { hintVisibility } from "../overlay/Overlay";
import { answerTurns, keepFirst, moveItem, removeItem, setMeSpeaker, shuffleItems } from "../state/practice";
import { Practice } from "./Practice";
import { Review } from "./Review";

async function flush() {
  for (let i = 0; i < 5; i++) {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
  }
}

function subtitles(): PracticeScript {
  const line = (id: number, speaker: string, text: string) => ({
    id,
    text,
    speaker,
    startMs: id * 1000,
    endMs: id * 1000 + 800,
    isMe: false,
    isQuestion: text.endsWith("?"),
    reference: null,
    category: null,
    followups: [],
  });
  return {
    kind: "subtitles",
    title: "talk",
    items: [line(1, "A", "Hi?"), line(2, "B", "Hello."), line(3, "B", "More."), line(4, "A", "Why?"), line(5, "B", "Because.")],
    speakers: ["A", "B"],
    meSpeaker: null,
    needsClassification: false,
  };
}

describe("practice script editing", () => {
  it("moves, removes, keeps and renumbers", () => {
    const s = mockPracticeScript();
    const moved = moveItem(s, 0, 1);
    expect(moved.items.map((i) => i.id)).toEqual([1, 2, 3]);
    expect(moved.items[1].text).toBe(s.items[0].text);
    expect(moveItem(s, 0, -1)).toBe(s);
    expect(removeItem(s, 1).items.map((i) => i.text)).toEqual([s.items[0].text, s.items[2].text]);
    expect(keepFirst(s, 2).items).toHaveLength(2);
    expect(keepFirst(s, 0).items).toHaveLength(3);
  });

  it("shuffles with the given random source", () => {
    const s = mockPracticeScript();
    const shuffled = shuffleItems(s, () => 0);
    expect(shuffled.items.map((i) => i.text)).toEqual([s.items[1].text, s.items[2].text, s.items[0].text]);
    expect(shuffled.items.map((i) => i.id)).toEqual([1, 2, 3]);
  });

  it("counts answer turns", () => {
    expect(answerTurns(mockPracticeScript())).toBe(3);
    const s = setMeSpeaker(subtitles(), "B");
    expect(s.items.filter((i) => i.isMe)).toHaveLength(3);
    expect(answerTurns(s)).toBe(2);
    expect(answerTurns(setMeSpeaker(s, null))).toBe(0);
  });
});

describe("hint levels", () => {
  it("hides, delays or shows hints", () => {
    expect(hintVisibility({ level: "full", delayS: 5 }, 0, 0).hidden).toBe(false);
    expect(hintVisibility({ level: "off", delayS: 5 }, 0, 99_000).hidden).toBe(true);
    expect(hintVisibility({ level: "delayed", delayS: 5 }, 0, 1_200)).toEqual({ hidden: true, waitS: 4 });
    expect(hintVisibility({ level: "delayed", delayS: 5 }, 0, 5_000).hidden).toBe(false);
  });
});

describe("Practice window", () => {
  beforeEach(() => {
    setLanguage("en");
    setBackend(createMockBackend());
  });

  it("imports, edits and starts a practice", async () => {
    render(<Practice />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Choose a file…" }));
    await flush();
    expect(screen.getByText("3 questions", { exact: false })).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Delete" })[0]);
    await flush();
    expect(screen.getByText("2 questions", { exact: false })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /Simulated latency/ }));
    await flush();
    expect(screen.getByText("Only recognition delay is simulated; the model's delay is real.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start practice" }));
    await flush();
    expect(screen.getByRole("heading", { name: "Practice is running" })).toBeInTheDocument();
    expect(screen.getByText("Question 1 / 2")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "End practice" }));
    await flush();
    expect(screen.getByText("The review opens in its own window.")).toBeInTheDocument();
  });

  it("shows the review report in Chinese", async () => {
    const b = createMockBackend();
    setBackend(b);
    const settings = await b.invoke("get_settings");
    await b.invoke("save_settings", { settings: { ...settings, general: { ...settings.general, uiLanguage: "zh-CN" } } });
    render(<Review />);
    await flush();
    expect(screen.getByRole("heading", { name: "总览" })).toBeInTheDocument();
    expect(screen.getByText("识别延迟为模拟值，模型延迟为真实值。")).toBeInTheDocument();
    expect(screen.getByText("漏掉了故障收敛时间这个要点。")).toBeInTheDocument();
    expect(screen.getByText("212 字/分钟")).toBeInTheDocument();
  });
});
