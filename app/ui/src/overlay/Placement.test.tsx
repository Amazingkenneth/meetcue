import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { backend, setBackend } from "../backend";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { acceleratorFromEvent } from "../settings/Settings";
import { fontStack } from "../state/hooks";
import { Overlay } from "./Overlay";

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("Panel placement", () => {
  beforeEach(() => {
    setLanguage("en");
    setBackend(createMockBackend({ speed: 1 }));
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("snaps the panel from the Move menu", async () => {
    const invoke = vi.spyOn(backend(), "invoke");
    render(<Overlay />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Move" }));
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Bottom left" }));
    expect(invoke).toHaveBeenCalledWith("change_panel", { change: { type: "snap", place: "bottomLeft" } });
    // One screen: no "next screen" target.
    expect(screen.queryByTitle("Move to the next screen")).toBeNull();
  });

  it("saves a spot and says so", async () => {
    render(<Overlay />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: "Move" }));
    await flush();
    fireEvent.click(screen.getAllByRole("button", { name: "Save here" })[1]);
    await flush();
    expect(screen.getByText("Saved this spot as 2")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Go" })).toHaveLength(1);
  });

  it("drags from empty space but not from controls", async () => {
    const drag = vi.spyOn(backend(), "startDragging");
    render(<Overlay />);
    await flush();
    fireEvent.mouseDown(screen.getByText("Meeting prompter"), { button: 0, detail: 2 });
    expect(drag).toHaveBeenCalledTimes(1);
    fireEvent.mouseDown(screen.getByRole("button", { name: "Move" }), { button: 0 });
    expect(drag).toHaveBeenCalledTimes(1);
  });

  it("shows the way back while clicks pass through", async () => {
    render(<Overlay />);
    await flush();
    await act(async () => {
      await backend().invoke("change_panel", { change: { type: "clickThrough", on: true } });
    });
    expect(screen.getByText(/Clicks pass through the panel. Press Ctrl\+Alt\+Shift\+T/)).toBeInTheDocument();
  });
});

describe("Appearance helpers", () => {
  it("builds a font stack that falls back to the system font", () => {
    expect(fontStack("")).toBe("");
    expect(fontStack(" KaiTi ")).toBe('"KaiTi", var(--font-system)');
    expect(fontStack('Bad"; color: red')).toBe('"Bad color: red", var(--font-system)');
  });

  it("records digits for shortcuts even with Shift held", () => {
    const e = { ctrlKey: true, altKey: true, shiftKey: true, metaKey: false, key: "!", code: "Digit1" };
    expect(acceleratorFromEvent(e)).toBe("Ctrl+Alt+Shift+1");
  });
});
