#!/usr/bin/env node
// Checks that every UI language has the same keys and no empty strings.
// Plural suffixes (_one, _other, …) are compared by their base key, since
// languages have different plural categories.

import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "app", "ui", "locales");
const PLURAL = /_(zero|one|two|few|many|other)$/;

function flatten(obj, prefix = "", out = new Map()) {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object") flatten(v, key, out);
    else out.set(key, v);
  }
  return out;
}

const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
const langs = files.map((f) => ({ name: f, keys: flatten(JSON.parse(readFileSync(join(dir, f), "utf8"))) }));
let failed = false;
const base = (k) => k.replace(PLURAL, "");

for (const l of langs) {
  for (const [k, v] of l.keys) {
    if (typeof v !== "string" || v.trim() === "") {
      console.error(`${l.name}: empty value for "${k}"`);
      failed = true;
    }
  }
}

const sets = langs.map((l) => ({ name: l.name, keys: new Set([...l.keys.keys()].map(base)) }));
for (const a of sets) {
  for (const b of sets) {
    if (a === b) continue;
    for (const k of a.keys) {
      if (!b.keys.has(k)) {
        console.error(`${b.name}: missing "${k}" (present in ${a.name})`);
        failed = true;
      }
    }
  }
}

// Every plural base needs an "other" form.
for (const l of langs) {
  for (const k of l.keys.keys()) {
    if (PLURAL.test(k) && !l.keys.has(`${base(k)}_other`)) {
      console.error(`${l.name}: "${base(k)}" has plural forms but no _other`);
      failed = true;
    }
  }
}

// Placeholders must match between languages.
const vars = (s) => [...String(s).matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort().join(",");
const [first, ...rest] = langs;
for (const l of rest) {
  for (const [k, v] of l.keys) {
    const other = first.keys.get(k) ?? first.keys.get(`${base(k)}_other`);
    if (other !== undefined && vars(other) !== vars(v)) {
      console.error(`${l.name}: placeholders of "${k}" differ from ${first.name}`);
      failed = true;
    }
  }
}

if (failed) process.exit(1);
console.log(`Locales OK: ${files.join(", ")} (${sets[0].keys.size} keys)`);
