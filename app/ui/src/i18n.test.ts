import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import en from "../locales/en.json";
import zh from "../locales/zh-CN.json";
import i18n, { resolveLanguage, setLanguage } from "./i18n";

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === "bindings" ? [] : files(p);
    return /\.(ts|tsx)$/.test(f) && !f.endsWith(".test.ts") ? [p] : [];
  });
}

function has(obj: unknown, key: string): boolean {
  let cur: unknown = obj;
  for (const part of key.split(".")) {
    if (!cur || typeof cur !== "object") return false;
    const o = cur as Record<string, unknown>;
    if (part in o) cur = o[part];
    else if (`${part}_other` in o) return true;
    else return false;
  }
  return typeof cur === "string";
}

describe("i18n", () => {
  it("defines every literal key used in the code in both languages", () => {
    const missing: string[] = [];
    for (const f of files(join(__dirname))) {
      const src = readFileSync(f, "utf8");
      for (const m of src.matchAll(/\bt\(\s*"([a-zA-Z0-9_.]+)"/g)) {
        for (const [name, dict] of [
          ["en", en],
          ["zh-CN", zh],
        ] as const) {
          if (!has(dict, m[1])) missing.push(`${name}: ${m[1]} (${f})`);
        }
      }
    }
    expect(missing).toEqual([]);
  });

  it("resolves the language from settings or the system", () => {
    expect(resolveLanguage(null, "zh-Hans-CN")).toBe("zh-CN");
    expect(resolveLanguage(null, "de-DE")).toBe("en");
    expect(resolveLanguage("en", "zh-CN")).toBe("en");
  });

  it("switches language immediately", () => {
    setLanguage("zh-CN");
    expect(i18n.t("overlay.covered", { done: 2, total: 4 })).toBe("已讲 2/4");
    setLanguage("en");
    expect(i18n.t("overlay.covered", { done: 2, total: 4 })).toBe("2/4 covered");
    expect(i18n.t("overlay.pendingCount", { count: 1 })).toBe("1 waiting");
  });
});
