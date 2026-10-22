import { useCallback } from "react";
import type { Settings } from "../bindings/Settings";
import { useSettings } from "../state/hooks";

/** Settings plus an `update` that applies a change and saves immediately. */
export function useDraft(): { settings: Settings | null; update: (f: (s: Settings) => Settings) => Promise<void> } {
  const [settings, save] = useSettings();
  const update = useCallback(
    async (f: (s: Settings) => Settings) => {
      if (!settings) return;
      await save(f(structuredClone(settings)));
    },
    [settings, save],
  );
  return { settings, update };
}
