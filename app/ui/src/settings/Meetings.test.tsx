import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import { setBackend } from "../backend";
import type { Settings } from "../bindings/Settings";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { MeetingsTab } from "./Meetings";

function harness() {
  const b = createMockBackend({ speed: 1000 });
  setBackend(b);
  let settings: Settings | null = null;
  const update = async (f: (s: Settings) => Settings) => {
    settings = f(structuredClone(settings ?? (await b.invoke("get_settings"))));
    await b.invoke("save_settings", { settings });
  };
  return { b, update, current: () => settings };
}

it("lists saved meetings and opens a recap with ticked and missed points", async () => {
  setLanguage("en");
  window.location.hash = "";
  const { b, update } = harness();
  render(<MeetingsTab settings={await b.invoke("get_settings")} update={update} />);
  const open = await screen.findByRole("button", { name: /\d/ });
  fireEvent.click(open);
  expect(await screen.findByText("1. Network high availability")).toBeInTheDocument();
  expect(screen.getByText(/3\/5 points covered/)).toBeInTheDocument();
  expect(screen.getAllByLabelText("Not covered")).toHaveLength(2);
});

it("turns saving off and keeps the transcript switch off by default", async () => {
  setLanguage("en");
  window.location.hash = "";
  const { b, update, current } = harness();
  const s = await b.invoke("get_settings");
  expect(s.history).toEqual({ saveMeetings: true, saveTranscript: false, keepDays: 90 });
  render(<MeetingsTab settings={s} update={update} />);
  fireEvent.click(screen.getByRole("switch", { name: /Save a recap/ }));
  await waitFor(() => expect(current()?.history.saveMeetings).toBe(false));
});

it("saves the preview meeting when listening stops", async () => {
  const { b } = harness();
  const saved: string[] = [];
  await b.onAppEvent((e) => {
    if (e.type === "meetingSaved") saved.push(e.id);
  });
  await b.invoke("start_session");
  await b.invoke("user_action", { action: { type: "ask", text: "How would you design a rate limiter?" } });
  await b.invoke("stop_session");
  expect(saved).toHaveLength(1);
  const m = await b.invoke("get_meeting", { id: saved[0] });
  expect(m.questions.map((q) => q.text)).toContain("How would you design a rate limiter?");
  expect(m.transcript).toEqual([]);
});
