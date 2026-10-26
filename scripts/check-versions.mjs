#!/usr/bin/env node
// The version lives in the root Cargo.toml ([workspace.package]); every
// other manifest must match it. scripts/set-version.mjs writes them all. tauri.conf.json inherits it from Cargo.

import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const cargo = read("Cargo.toml").match(/\[workspace\.package\][^[]*?version\s*=\s*"([^"]+)"/s);
if (!cargo) {
  console.error("Cargo.toml: no [workspace.package] version");
  process.exit(1);
}
const version = cargo[1];
let failed = false;

for (const p of ["package.json", "app/ui/package.json", "docs/package.json"]) {
  const v = JSON.parse(read(p)).version;
  if (v !== version) {
    console.error(`${p}: version ${v}, expected ${version}`);
    failed = true;
  }
}

const tauri = JSON.parse(read("app/src-tauri/tauri.conf.json"));
if (tauri.version !== undefined) {
  console.error("app/src-tauri/tauri.conf.json: remove \"version\"; it comes from Cargo.toml");
  failed = true;
}

const lock = JSON.parse(read("package-lock.json"));
for (const [key, v] of [["(root)", lock.version], ...["", "app/ui", "docs"].map((k) => [k || "(workspace)", lock.packages[k]?.version])]) {
  if (v !== version) {
    console.error(`package-lock.json ${key}: version ${v}, expected ${version}`);
    failed = true;
  }
}

for (const [, name, v] of read("Cargo.lock").matchAll(/name = "(meetcue-[a-z]+)"\nversion = "([^"]+)"/g)) {
  if (v !== version) {
    console.error(`Cargo.lock ${name}: version ${v}, expected ${version}`);
    failed = true;
  }
}

// The release workflow commits the version and the Scoop manifest together,
// so the manifest always names the committed version.
const scoop = JSON.parse(read("bucket/meetcue.json")).version;
if (scoop !== version) {
  console.error(`bucket/meetcue.json: version ${scoop}, expected ${version}`);
  failed = true;
}

if (failed) process.exit(1);
console.log(`Versions OK: ${version}`);
