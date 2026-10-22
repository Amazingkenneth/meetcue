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
    const queue = screen.getByRole("navigation", { name: "Question queue" });
    expect(within(queue).getByText("SDN 控制器部署")).toBeInTheDocument();
    expect(within(queue).getByText("控制器故障还能转发吗")).toBeInTheDocument();
    expect(within(queue).getByText("Waiting")).toBeInTheDocument();
    // The first question was fully covered and answered.
    const blocks = screen.getAllByRole("article");
    expect(blocks).toHaveLength(3);
    expect(within(blocks[0]).getByText("4/4 covered")).toBeInTheDocument();
    expect(within(blocks[0]).getAllByLabelText("Covered")).toHaveLength(4);
    // The active question highlights exactly one next point.
    expect(within(blocks[1]).getAllByText("Next", { selector: ".next-tag" })).toHaveLength(1);
    expect(within(blocks[2]).getByText("Queued")).toBeInTheDocument();

    // Full answer with bold keywords.
    fireEvent.click(within(blocks[0]).getByRole("button", { name: "Show full answer →" }));
    expect(within(blocks[0]).getByText("双活数据中心", { selector: "strong" })).toBeInTheDocument();
    // Context disclosure with tags.
    fireEvent.click(within(blocks[1]).getByRole("button", { name: /SDN 控制器是怎么部署/ }));
    expect(within(blocks[1]).getByText("复合问题·两问")).toBeInTheDocument();

    // Latency in the footer.
    expect(screen.getByRole("button", { name: /^Latency \d\.\d s$/ })).toBeInTheDocument();
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
