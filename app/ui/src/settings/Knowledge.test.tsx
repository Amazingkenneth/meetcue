import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";
import { setBackend } from "../backend";
import type { Settings } from "../bindings/Settings";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { KnowledgeSection, missingPlaceholders, PromptsSection } from "./Knowledge";

function harness() {
  const b = createMockBackend();
  setBackend(b);
  let settings: Settings | null = null;
  const update = async (f: (s: Settings) => Settings) => {
    settings = f(structuredClone(settings ?? (await b.invoke("get_settings"))));
    await b.invoke("save_settings", { settings });
  };
  return { b, update, current: () => settings };
}

it("adds files and reports the ones that can't be read", async () => {
  setLanguage("en");
  const { b, update, current } = harness();
  render(<KnowledgeSection settings={await b.invoke("get_settings")} update={update} />);
  fireEvent.click(screen.getByRole("button", { name: "Add files…" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("old-notes.doc");
  expect(alert).toHaveTextContent("The knowledge base file could not be read");
  await waitFor(() => expect(current()?.hints.materials.map((m) => m.name)).toEqual(["agenda.md"]));
});

it("shows the built-in prompts and saves edits as custom", async () => {
  setLanguage("en");
  const { b, update, current } = harness();
  render(<PromptsSection settings={await b.invoke("get_settings")} update={update} />);
  const box = await screen.findByLabelText(/Question message/);
  expect((box as HTMLTextAreaElement).value).toContain("{knowledge}");
  fireEvent.change(box, { target: { value: "Q: {question}" } });
  await waitFor(() => expect(current()?.hints.prompts.hintsUser).toBe("Q: {question}"));
});

it("finds missing placeholders", () => {
  expect(missingPlaceholders("hintsUser", "Q: {question} {transcript}")).toEqual(["{knowledge}"]);
  expect(missingPlaceholders("hintsSystem", "{role} {materials} {output_language}")).toEqual([]);
});
