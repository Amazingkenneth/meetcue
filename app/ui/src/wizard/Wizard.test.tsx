import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
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

/** Any provider and any model name: nothing comes from a built-in list. */
async function chooseAll() {
  const pick = async (label: string, value: string) => {
    fireEvent.change(screen.getByLabelText(label), { target: { value } });
    await flush();
  };
  const [asrProvider, detectProvider, hintsProvider] = screen.getAllByLabelText("Provider");
  const models = () => screen.getAllByLabelText("Model");
  fireEvent.change(asrProvider, { target: { value: "soniox" } });
  await flush();
  fireEvent.change(models()[0], { target: { value: "stt-rt-v5" } });
  await flush();
  fireEvent.change(detectProvider, { target: { value: "openai-compatible" } });
  await flush();
  fireEvent.change(models()[1], { target: { value: "my-local-model" } });
  await flush();
  fireEvent.change(hintsProvider, { target: { value: "openai-compatible" } });
  await flush();
  fireEvent.change(models()[2], { target: { value: "another-model" } });
  await flush();
  await pick("OpenAI-compatible base URL", "http://localhost:11434/v1");
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
    await chooseAll();
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
    expect(screen.getByLabelText("Soniox key")).toBeInTheDocument();
    expect(screen.getByLabelText("OpenAI-compatible key")).toBeInTheDocument();
    expect(screen.getByLabelText("OpenAI-compatible base URL")).toBeInTheDocument();
    expect(screen.queryByLabelText("Anthropic (Claude) key")).not.toBeInTheDocument();
  });

  it("closes its window through the backend when finished", async () => {
    const b = createMockBackend();
    const close = vi.spyOn(b, "closeWindow").mockResolvedValue();
    setBackend(b);
    render(<Wizard />);
    await flush();
    const next = async () => {
      fireEvent.click(screen.getByRole("button", { name: "Next" }));
      await flush();
    };
    await next();
    await next();
    await chooseAll();
    await next();
    fireEvent.click(screen.getByRole("button", { name: "Skip" }));
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await flush();
    expect(close).toHaveBeenCalledOnce();
    expect((await b.invoke("get_settings")).general.wizardCompleted).toBe(true);
  });
});
