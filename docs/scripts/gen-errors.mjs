#!/usr/bin/env node
// Generates the error code reference pages from crates/core/data/errors.json,
// the same file the app uses, so the two never disagree.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const docs = join(dirname(fileURLToPath(import.meta.url)), "..");
const catalog = JSON.parse(readFileSync(join(docs, "..", "crates/core/data/errors.json"), "utf8"));

const pages = {
  en: {
    path: "troubleshooting/error-codes.md",
    title: "Error codes",
    lead:
      "Every message in MeetCue shows a code such as `MC-AUD-001`. Find the code below for what it means and how to fix it. The **See the fix** button in the app opens this page at the right code.",
    severity: { error: "Error", warning: "Warning", info: "Notice" },
    fix: "How to fix",
    more: "Still stuck? [Export diagnostics](./diagnostics) and open an issue with the code.",
    modules: { AUD: "Audio", ASR: "Speech recognition", LLM: "Language models", HK: "Shortcuts", SYS: "System", NET: "Network", BUD: "Budget", PRC: "Practice", DIAG: "Diagnostics" },
    lang: "en",
  },
  zh: {
    path: "zh/troubleshooting/error-codes.md",
    title: "错误码参考",
    lead: "MeetCue 的每条提示都带有一个错误码，例如 `MC-AUD-001`。在下面找到对应的错误码，查看含义与解决办法。应用里的“查看解决办法”按钮会直接打开本页对应的位置。",
    severity: { error: "错误", warning: "警告", info: "提示" },
    fix: "解决办法",
    more: "仍未解决？请[导出诊断信息](./diagnostics)，并带上错误码提交 Issue。",
    modules: { AUD: "音频", ASR: "语音识别", LLM: "大模型", HK: "快捷键", SYS: "系统", NET: "网络", BUD: "预算", PRC: "练习", DIAG: "诊断" },
    lang: "zh-CN",
  },
};

for (const p of Object.values(pages)) {
  let md = `<!-- Generated from crates/core/data/errors.json by docs/scripts/gen-errors.mjs. Do not edit. -->\n\n# ${p.title}\n\n${p.lead}\n`;
  let module = "";
  for (const e of catalog.errors) {
    const m = e.code.split("-")[1];
    if (m !== module) {
      module = m;
      md += `\n## ${p.modules[m] ?? m}\n`;
    }
    md += `\n### ${e.code} {#${e.code.toLowerCase()}}\n\n`;
    md += `**${e.title[p.lang]}** · ${p.severity[e.severity]}\n\n`;
    md += `${p.fix}: ${e.fix[p.lang]}\n`;
  }
  md += `\n---\n\n${p.more}\n`;
  const out = join(docs, p.path);
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, md);
}
console.log(`Generated error code pages (${catalog.errors.length} codes)`);
