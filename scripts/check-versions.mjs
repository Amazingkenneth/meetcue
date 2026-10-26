#!/usr/bin/env node
// The version lives in the root Cargo.toml ([workspace.package]); every
// other manifest must match it. tauri.conf.json inherits it from Cargo.

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

// bucket/meetcue.json is not compared: releases take their version from the
// tag, so the Scoop manifest is usually ahead of the version committed here.

if (failed) process.exit(1);
console.log(`Versions OK: ${version}`);
