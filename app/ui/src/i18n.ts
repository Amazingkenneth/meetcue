import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../locales/en.json";
import zhCN from "../locales/zh-CN.json";

export const LANGUAGES = ["en", "zh-CN"] as const;
export type Language = (typeof LANGUAGES)[number];

/** `zh*` system locales use Simplified Chinese, everything else English. */
export function resolveLanguage(setting: string | null | undefined, systemLocale: string): Language {
  if (setting === "en" || setting === "zh-CN") return setting;
  return systemLocale.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, "zh-CN": { translation: zhCN } },
  lng: resolveLanguage(null, typeof navigator !== "undefined" ? navigator.language : "en"),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  returnNull: false,
});

export function setLanguage(lang: Language): void {
  if (i18n.language !== lang) void i18n.changeLanguage(lang);
  document.documentElement.lang = lang;
}

export default i18n;
