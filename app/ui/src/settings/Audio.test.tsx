import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { setBackend } from "../backend";
import { setLanguage } from "../i18n";
import { createMockBackend } from "../mock/mockBackend";
import { AudioFields } from "./Settings";

it("shows macOS recording permission guidance when the level test fails", async () => {
  setLanguage("en");
  const b = createMockBackend();
  setBackend({ ...b, invoke: new Proxy(b.invoke, {
    apply(target, thisArg, args) {
      if (args[0] === "audio_preview" && args[1].on) return Promise.reject({ code: "MC-AUD-006" });
      return Reflect.apply(target, thisArg, args);
    },
  }) });
  const settings = await b.invoke("get_settings");
  render(<AudioFields settings={settings} update={async () => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "Test levels" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("System audio recording permission is missing");
  expect(alert).toHaveTextContent("Privacy & Security");
  expect(alert).toHaveTextContent("Quit and reopen MeetCue");
});
