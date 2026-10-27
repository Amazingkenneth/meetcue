import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { setBackend } from "../backend";
import type { PracticeScript } from "../bindings/PracticeScript";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { mockPracticeScript } from "../mock/practice";
import { hintVisibility } from "../overlay/Overlay";
import {
  addQuestion,
  answerTurns,
  groups,
  keepFirst,
  keepSelected,
  moveItem,
  moveSelected,
  range,
  removeItem,
  removeSelected,
  setGroup,
  setMeSpeaker,
  shuffleItems,
} from "../state/practice";
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

describe("batch changes", () => {
  it("deletes, keeps, moves and regroups the selection", () => {
    const s = addQuestion(mockPracticeScript(), "  Fourth?  ");
    expect(s.items.map((i) => i.id)).toEqual([1, 2, 3, 4]);
    expect(s.items[3].text).toBe("Fourth?");
    expect(addQuestion(s, " ")).toBe(s);
    const sel = new Set([1, 3]);
    expect(removeSelected(s, sel).items.map((i) => i.text)).toEqual([s.items[0].text, s.items[2].text]);
    expect(keepSelected(s, sel).items.map((i) => i.id)).toEqual([1, 2]);
    expect(keepSelected(s, new Set())).toBe(s);
    const top = moveSelected(s, sel, "top");
    expect(top.items.map((i) => i.text)).toEqual([s.items[1].text, "Fourth?", s.items[0].text, s.items[2].text]);
    expect(top.items.map((i) => i.id)).toEqual([1, 2, 3, 4]);
    expect(moveSelected(s, new Set([0]), "bottom").items[3].text).toBe(s.items[0].text);
    const g = setGroup(s, sel, " Design ");
    expect(g.items.map((i) => i.category)).toEqual(["SDN", "Design", "SDN", "Design"]);
    expect(groups(g)).toEqual(["SDN", "Design"]);
    expect(setGroup(s, sel, "").items[1].category).toBeNull();
    expect(range(4, 2)).toEqual([2, 3, 4]);
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
    fireEvent.click(screen.getByRole("button", { name: "Next: choose questions" }));
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Choose a file…" }));
    await flush();
    expect(screen.getByText("3 questions", { exact: false })).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Delete" })[0]);
    await flush();
    expect(screen.getByText("2 questions", { exact: false })).toBeInTheDocument();
    // Tick both rows (shift-click selects the range) and move them to the bottom.
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 1" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 2" }), { shiftKey: true });
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Add question" }), { target: { value: "My own question?" } });
    fireEvent.click(screen.getByRole("button", { name: "Add question" }));
    expect(screen.getByText("3 questions", { exact: false })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select 3" }));
    fireEvent.click(screen.getByRole("button", { name: "Keep only these" }));
    expect(screen.getByText("1 question", { exact: false })).toBeInTheDocument();
    expect(screen.getByDisplayValue("My own question?")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Choose another file" }));
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Choose a file…" }));
    await flush();
    fireEvent.click(screen.getAllByRole("button", { name: "Delete" })[0]);
    await flush();
    fireEvent.click(screen.getByRole("switch", { name: /Speech-to-text for the questions/ }));
    await flush();
    expect(screen.getByText("Only recognition delay is simulated; the model's delay is real.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start practice" }));
    await flush();
    expect(screen.getByRole("heading", { name: "Practice is running" })).toBeInTheDocument();
    expect(screen.getByText("Question 1 / 2")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Go to a question" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Go" }));
    await flush();
    expect(screen.getByText("Question 2 / 2")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Ask again" })[0]);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "End practice" }));
    await flush();
    expect(screen.getByText("The review opens in its own window.")).toBeInTheDocument();
  });

  it("checks the sound, picks a voice and starts from a built-in sample", async () => {
    const b = createMockBackend();
    setBackend(b);
    render(<Practice />);
    await flush();
    expect(screen.getByRole("heading", { name: "Check what MeetCue hears" })).toBeInTheDocument();
    expect(screen.getByRole("meter", { name: "System sound · Others" })).toBeInTheDocument();
    const voice = screen.getByLabelText("English voice") as HTMLSelectElement;
    fireEvent.change(voice, { target: { value: "mock-en" } });
    await flush();
    expect(voice.value).toBe("mock-en");
    expect((await b.invoke("get_settings")).practice.voiceEn).toBe("mock-en");
    fireEvent.click(screen.getAllByRole("button", { name: "Try it" })[0]);
    fireEvent.click(screen.getByRole("button", { name: "Next: choose questions" }));
    await flush();
    expect(screen.getAllByRole("button", { name: "Use this" })).toHaveLength(4);
    expect(screen.getAllByText("Chinese")).toHaveLength(2);
    fireEvent.click(screen.getAllByRole("button", { name: "Use this" })[1]);
    await flush();
    expect(screen.getByText("questions-security-ai")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start practice" })).toBeEnabled();
    expect(screen.getByText(/the first question is read aloud/)).toBeInTheDocument();
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
