#!/usr/bin/env node
// Every English page needs a Chinese page at /zh/ with the same number of
// second-level headings, so the two languages stay in step.

import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, relative, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const docs = join(dirname(fileURLToPath(import.meta.url)), "..");
const skip = new Set(["zh", "node_modules", ".vitepress", "public", "scripts"]);

function pages(dir) {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return skip.has(f) ? [] : pages(p);
    return f.endsWith(".md") ? [p] : [];
  });
}

const h2 = (text) => text.split("\n").filter((l) => /^## /.test(l)).length;
let failed = false;
for (const en of pages(docs)) {
  const rel = relative(docs, en);
  const zh = join(docs, "zh", rel);
  if (!existsSync(zh)) {
    console.error(`missing Chinese page: zh/${rel}`);
    failed = true;
    continue;
  }
  const a = h2(readFileSync(en, "utf8"));
  const b = h2(readFileSync(zh, "utf8"));
  if (a !== b) {
    console.error(`${rel}: ${a} second-level headings in English, ${b} in Chinese`);
    failed = true;
  }
}
for (const zh of pages(join(docs, "zh"))) {
  const rel = relative(join(docs, "zh"), zh);
  if (!existsSync(join(docs, rel))) {
    console.error(`Chinese page without English page: zh/${rel}`);
    failed = true;
  }
}
if (failed) process.exit(1);
console.log("Docs i18n OK");
