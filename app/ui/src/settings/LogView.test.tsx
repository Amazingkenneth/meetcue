import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { backend, setBackend } from "../backend";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { LogView, parseLine } from "./LogView";

describe("activity log", () => {
  beforeEach(() => {
    setLanguage("en");
    setBackend(createMockBackend({ speed: 50 }));
  });

  it("splits shell log lines into time, level and message", () => {
    const p = parseLine("2026-10-24T22:10:00.123456Z  WARN meetcue::session: code=MC-ASR-005 no key for soniox");
    expect(p.level).toBe("WARN");
    expect(p.message).toBe("code=MC-ASR-005 no key for soniox");
    expect(p.time).toMatch(/^\d{2}:\d{2}:\d{2}$/);
    expect(parseLine("plain text").message).toBe("plain text");
  });

  it("shows what the session does as it happens", async () => {
    render(<LogView />);
    await act(async () => {
      await backend().invoke("toggle_capture");
      await new Promise((r) => setTimeout(r, 1200));
    });
    expect(screen.getByRole("log")).toHaveTextContent("session running");
  });
});
