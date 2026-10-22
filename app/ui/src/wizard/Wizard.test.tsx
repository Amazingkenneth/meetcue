import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { setBackend } from "../backend";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { Wizard } from "./Wizard";

async function flush() {
  for (let i = 0; i < 5; i++) {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
  }
}

describe("Wizard", () => {
  beforeEach(() => {
    setLanguage("en");
    setBackend(createMockBackend());
  });

  it("requires choosing all three models before continuing", async () => {
    render(<Wizard />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await flush();
    expect(screen.getByRole("heading", { name: "Audio" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await flush();
    expect(screen.getByText("Choose a model for each of the three tasks to continue.")).toBeInTheDocument();
    const next = screen.getByRole("button", { name: "Next" });
    expect(next).toBeDisabled();
    for (const name of ["Use Soniox stt-rt-v5"]) {
      fireEvent.click(screen.getByRole("radio", { name }));
      await flush();
    }
    const haiku = screen.getAllByRole("radio", { name: "Use Claude Haiku 5.5" });
    expect(haiku).toHaveLength(2);
    for (const r of haiku) {
      fireEvent.click(r);
      await flush();
    }
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await flush();
    expect(screen.getByLabelText("Soniox key")).toBeInTheDocument();
    expect(screen.getByLabelText("Anthropic (Claude) key")).toBeInTheDocument();
  });
});
