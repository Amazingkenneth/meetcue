import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setBackend } from "../backend";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { Overlay } from "./Overlay";

async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe("Overlay", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: false });
    setLanguage("en");
    setBackend(createMockBackend({ speed: 1 }));
    Element.prototype.scrollIntoView = vi.fn();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows how to start when idle", async () => {
    render(<Overlay />);
    await flush();
    expect(screen.getAllByText("Ctrl+Alt+Space", { selector: "kbd" }).length).toBeGreaterThan(0);
    expect(screen.getByText(/to start listening/)).toBeInTheDocument();
    expect(screen.getByText("Not listening")).toBeInTheDocument();
  });

  it("renders the replayed interview: newest first, points, coverage and answers", async () => {
    render(<Overlay />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    // Halfway: question 2 is active and question 3 waits in the queue.
    await act(async () => {
      vi.advanceTimersByTime(46_000);
    });
    // The newest question is shown in full on top, even while it waits its turn.
    const blocks = screen.getAllByRole("article");
    expect(blocks).toHaveLength(1);
    expect(within(blocks[0]).getByText("控制器挂了还能转发吗？")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back to the latest question" })).not.toBeInTheDocument();
    // The list is in time order, newest first, and marks the question shown above.
    const list = () =>
      within(screen.getByRole("navigation", { name: "Questions, newest first" }))
        .getAllByRole("button")
        .filter((b) => b.classList.contains("ov-row"));
    const rows = list();
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("控制器故障还能转发吗");
    expect(rows[0]).toHaveTextContent("Waiting");
    expect(rows[0]).toHaveAttribute("aria-current", "true");
    expect(rows[1]).toHaveTextContent("SDN 控制器部署");
    expect(rows[1]).toHaveTextContent("Now");
    expect(rows[2]).toHaveTextContent("Answered");
    expect(rows[2]).toHaveTextContent("4/4 covered");

    // Clearing hides the answered question from the list until restored; the others stay.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Clear earlier" }));
    });
    expect(list()).toHaveLength(2);
    expect(screen.getByText("1 cleared")).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    });
    expect(list()).toHaveLength(3);

    // Opening the active question shows it in the same place, with a way back.
    await act(async () => {
      fireEvent.click(list()[1]);
    });
    const active = screen.getByRole("article");
    expect(active).toHaveAttribute("aria-current", "true");
    expect(within(active).getAllByText("Next", { selector: ".next-tag" })).toHaveLength(1);
    expect(within(active).getByRole("button", { name: "Back to the latest question" })).toBeInTheDocument();
    // Context disclosure with tags.
    fireEvent.click(within(active).getByRole("button", { name: /SDN 控制器是怎么部署/ }));
    expect(within(active).getByText("复合问题·两问")).toBeInTheDocument();
    // The list keeps its order while another question is shown.
    expect(list()[0]).toHaveTextContent("控制器故障还能转发吗");
    expect(list()[1]).toHaveAttribute("aria-current", "true");

    // The answered question, with its full answer and bold keywords.
    await act(async () => {
      fireEvent.click(list()[2]);
    });
    const answered = screen.getByRole("article");
    expect(within(answered).getAllByLabelText("Covered")).toHaveLength(4);
    fireEvent.click(within(answered).getByRole("button", { name: "Show full answer →" }));
    expect(within(answered).getByText("双活数据中心", { selector: "strong" })).toBeInTheDocument();
    // One click goes back to the newest question.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Back to the latest question" }));
    });
    expect(screen.queryByRole("button", { name: "Back to the latest question" })).not.toBeInTheDocument();
    expect(list()[0]).toHaveAttribute("aria-current", "true");

    // Latency in the footer.
    expect(screen.getByRole("button", { name: /^Latency \d\.\d s$/ })).toBeInTheDocument();
  });

  it("ticks and unticks a point by click", async () => {
    render(<Overlay />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await act(async () => {
      vi.advanceTimersByTime(46_000);
    });
    const active = screen.getAllByRole("article")[0];
    const before = within(active).queryAllByLabelText("Covered").length;
    const point = within(active).getAllByRole("button", { pressed: false })[0];
    await act(async () => {
      fireEvent.click(point);
    });
    expect(within(active).getAllByLabelText("Covered")).toHaveLength(before + 1);
    await act(async () => {
      fireEvent.click(within(active).getAllByRole("button", { pressed: true })[0]);
    });
    expect(within(active).queryAllByLabelText("Covered")).toHaveLength(before);
  });

  it("switches language without reloading", async () => {
    render(<Overlay />);
    await flush();
    expect(screen.getByText("Meeting prompter")).toBeInTheDocument();
    await act(async () => setLanguage("zh-CN"));
    expect(screen.getByText("会议提词")).toBeInTheDocument();
    expect(screen.getByText("系统声音 · 与会者")).toBeInTheDocument();
    await act(async () => setLanguage("en"));
  });

  it("switches to the mini bar and shows the next point large", async () => {
    render(<Overlay />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await act(async () => {
      vi.advanceTimersByTime(36_000);
    });
    fireEvent.click(screen.getByRole("button", { name: "Mini" }));
    await flush();
    expect(screen.getByRole("button", { name: "Expand" })).toBeInTheDocument();
    expect(screen.getByText("SDN 控制器部署")).toBeInTheDocument();
  });
});

describe("footerKeys", () => {
  it("shows the shared modifier once", async () => {
    const { footerKeys } = await import("./Overlay");
    expect(footerKeys()).toEqual({ prefix: "Ctrl+Alt+", toggle: "Space", focus: "[ ]", answered: "Enter" });
    expect(footerKeys("Ctrl+Shift+K").prefix).toBe("");
  });
});

describe("compactRows", () => {
  it("keeps the shown, live and waiting questions, folds older ones and leaves cleared ones out", async () => {
    const { compactRows, RECENT_ROWS } = await import("./Overlay");
    const { emptySession, timeline } = await import("../state/session");
    const view = emptySession();
    const status = (id: number) => (id === 9 ? "pending" : id === 4 ? "active" : "answered");
    for (let id = 1; id <= 9; id++) {
      view.questions[id] = {
        id,
        version: 1,
        text: `q${id}`,
        short: `q${id}`,
        tags: [],
        subQuestions: [],
        status: status(id),
        tMs: id * 1000,
        context: [],
        points: [],
        answer: "",
        answerDone: true,
        failedCode: null,
        latencyMs: null,
      };
      view.order.push(id);
    }
    const all = timeline(view);
    expect(all).toEqual([9, 8, 7, 6, 5, 4, 3, 2, 1]);
    // Shown in full: question 2; live: question 4; waiting: question 9.
    const rows = compactRows(view, all, 2, 4, new Set([1]));
    expect(rows.keep).toEqual([9, 4, 2]);
    expect(rows.recent).toEqual([8, 7, 6].slice(0, RECENT_ROWS));
    expect(rows.older).toEqual([5, 3]);
    expect(rows.cleared).toBe(1);
    // Clearing never hides the shown, live or a waiting question.
    const cleared = compactRows(view, all, 2, 4, new Set(all));
    expect(cleared.keep).toEqual([9, 4, 2]);
    expect(cleared.earlier).toEqual([]);
  });
});
