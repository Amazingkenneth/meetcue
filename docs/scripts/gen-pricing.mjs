#!/usr/bin/env node
// Generates the price reference pages from crates/core/data/pricing.json.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const docs = join(dirname(fileURLToPath(import.meta.url)), "..");
const table = JSON.parse(readFileSync(join(docs, "..", "crates/core/data/pricing.json"), "utf8"));

const perHour = (e) => {
  const k = { per_audio_hour: 1, per_audio_minute: 60, per_audio_second: 3600 }[e.unit];
  return k ? e.audio * k : null;
};
const perMillion = (v, unit) => (v === undefined ? null : unit === "per_thousand_tokens" ? v * 1000 : v);
const money = (v) => (v === null || v === undefined ? "—" : `$${Number(v.toFixed(4))}`);

const pages = {
  en: {
    path: "reference/pricing.md",
    title: "Price reference",
    lead: `Prices MeetCue uses for its estimates, checked against the providers' pages on ${table.updated}. Prices change: the provider's website and your bill are authoritative. You can override any price in **Settings › Providers & cost**.`,
    speech: "Speech recognition",
    models: "Language models",
    cols: ["Provider", "Model", "Per audio hour", "Source"],
    lcols: ["Provider", "Model", "Input / M tokens", "Output / M tokens", "Cache read / M tokens", "Source"],
    link: "price page",
    how: "How the hourly estimate works",
    howText:
      "Speech recognition: price × 2 channels × share of time with speech (default 0.6). Models: questions per hour (default 20) × (one detection call + one hint call), with the static system prompt at the cache-read price, plus 10% for early triggers that are cancelled. All numbers can be changed in the settings.",
  },
  zh: {
    path: "zh/reference/pricing.md",
    title: "价格参考",
    lead: `MeetCue 估算费用时使用的价格，于 ${table.updated} 对照供应商官网核对。价格会变化，请以供应商官网和账单为准。你可以在 **设置 › 供应商与费用** 中覆盖任意价格。`,
    speech: "语音识别",
    models: "大模型",
    cols: ["供应商", "模型", "每音频小时", "来源"],
    lcols: ["供应商", "模型", "输入 / 百万 token", "输出 / 百万 token", "缓存读取 / 百万 token", "来源"],
    link: "价格页",
    how: "每小时费用如何估算",
    howText:
      "语音识别：单价 × 2 路 × 有效语音占比（默认 0.6）。模型：每小时问题数（默认 20）×（一次问题检测 + 一次要点生成），不变的系统提示按缓存读取价计算，另加 10% 作为提前触发被取消的损耗。所有假设都可以在设置中修改。",
  },
};

for (const p of Object.values(pages)) {
  const audio = table.entries.filter((e) => e.unit.startsWith("per_audio"));
  const llm = table.entries.filter((e) => !e.unit.startsWith("per_audio"));
  let md = `<!-- Generated from crates/core/data/pricing.json by docs/scripts/gen-pricing.mjs. Do not edit. -->\n\n# ${p.title}\n\n${p.lead}\n\n## ${p.speech}\n\n`;
  md += `| ${p.cols.join(" | ")} |\n| --- | --- | ---: | --- |\n`;
  for (const e of audio) md += `| ${e.provider} | ${e.label || e.model} | ${money(perHour(e))} | [${p.link}](${e.source}) |\n`;
  md += `\n## ${p.models}\n\n| ${p.lcols.join(" | ")} |\n| --- | --- | ---: | ---: | ---: | --- |\n`;
  for (const e of llm)
    md += `| ${e.provider} | ${e.label || e.model} | ${money(perMillion(e.input, e.unit))} | ${money(perMillion(e.output, e.unit))} | ${money(perMillion(e.cacheRead, e.unit))} | [${p.link}](${e.source}) |\n`;
  md += `\n## ${p.how}\n\n${p.howText}\n`;
  const out = join(docs, p.path);
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, md);
}
console.log(`Generated price reference (${table.entries.length} entries)`);
