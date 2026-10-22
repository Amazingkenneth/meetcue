import catalog from "../../../../crates/core/data/errors.json";
import type { ErrorInfo } from "../bindings/ErrorInfo";
import i18n from "../i18n";

const byCode = new Map<string, ErrorInfo>((catalog.errors as ErrorInfo[]).map((e) => [e.code, e]));

export const DOCS_PAGE: string = catalog.docsPage;

export function errorInfo(code: string): ErrorInfo | undefined {
  return byCode.get(code);
}

export function errorTitle(code: string): string {
  const e = byCode.get(code);
  if (!e) return code;
  return i18n.language.startsWith("zh") ? e.title["zh-CN"] : e.title.en;
}

export function errorFix(code: string): string {
  const e = byCode.get(code);
  if (!e) return "";
  return i18n.language.startsWith("zh") ? e.fix["zh-CN"] : e.fix.en;
}

/** Documentation link for a code in the current language. */
export function errorDocsUrl(docsBase: string, code: string): string {
  const base = docsBase.replace(/\/+$/, "");
  const prefix = i18n.language.startsWith("zh") ? "/zh" : "";
  return `${base}${prefix}/${DOCS_PAGE}#${code.toLowerCase()}`;
}
