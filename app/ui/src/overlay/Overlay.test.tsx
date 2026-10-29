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

  it("renders the replayed interview: queue, points, coverage and answers", async () => {
    render(<Overlay />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    // Halfway: question 2 is active and question 3 waits in the queue.
    await act(async () => {
      vi.advanceTimersByTime(46_000);
    });
    // Only the current question is shown in full; the others are one-line rows.
    const blocks = screen.getAllByRole("article");
    expect(blocks).toHaveLength(1);
    const [active] = blocks;
    expect(active).toHaveAttribute("aria-current", "true");
    expect(within(active).getByText(/SDN 控制器是怎么部署/)).toBeInTheDocument();
    // The active question highlights exactly one next point.
    expect(within(active).getAllByText("Next", { selector: ".next-tag" })).toHaveLength(1);
    const rows = within(screen.getByRole("navigation", { name: "Other questions" })).getAllByRole("button");
    expect(rows).toHaveLength(2);
    // The waiting question comes first, then the answered one.
    expect(rows[0]).toHaveTextContent("控制器故障还能转发吗");
    expect(rows[0]).toHaveTextContent("Waiting");
    expect(rows[1]).toHaveTextContent("Answered");
    expect(rows[1]).toHaveTextContent("4/4 covered");

    // Opening the answered question shows it in full, with a way back.
    await act(async () => {
      fireEvent.click(rows[1]);
    });
    const answered = screen.getByRole("article");
    expect(within(answered).getAllByLabelText("Covered")).toHaveLength(4);
    expect(screen.getByText("Looking at another question")).toBeInTheDocument();
    // Full answer with bold keywords.
    fireEvent.click(within(answered).getByRole("button", { name: "Show full answer →" }));
    expect(within(answered).getByText("双活数据中心", { selector: "strong" })).toBeInTheDocument();
    // One click goes back to the current question.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Back to the current question" }));
    });
    expect(screen.queryByText("Looking at another question")).not.toBeInTheDocument();
    const current = screen.getByRole("article");
    // Context disclosure with tags.
    fireEvent.click(within(current).getByRole("button", { name: /SDN 控制器是怎么部署/ }));
    expect(within(current).getByText("复合问题·两问")).toBeInTheDocument();

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
