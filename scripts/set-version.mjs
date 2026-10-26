#!/usr/bin/env node
// Writes a version into every manifest that carries one, so the release
// workflow never needs a hand-made version bump.
//
//   node scripts/set-version.mjs 0.2.0
//       Cargo.toml, Cargo.lock, the package.json files and package-lock.json.
//   node scripts/set-version.mjs 0.2.0 --changelog
//       Also moves the Unreleased entries of both changelogs under 0.2.0.
//   node scripts/set-version.mjs 0.2.0 --hash <sha256>
//       Also points bucket/meetcue.json at the 0.2.0 release with that hash.
//
// Files are edited in place, so their formatting stays as it is.

import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const write = (p, text) => writeFileSync(join(root, p), text);

const [version, ...rest] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("usage: set-version.mjs <major.minor.patch> [--changelog] [--hash <sha256>]");
  process.exit(1);
}
const changelog = rest.includes("--changelog");
const hashAt = rest.indexOf("--hash");
const hash = hashAt >= 0 ? rest[hashAt + 1] : undefined;
if (hashAt >= 0 && !/^[0-9a-f]{64}$/.test(hash ?? "")) {
  console.error("--hash needs a lowercase SHA-256");
  process.exit(1);
}

// Replaces exactly one match, so a changed file layout fails loudly
// instead of leaving a stale version behind.
function replaceOnce(path, text, pattern, replacement) {
  const matches = text.match(new RegExp(pattern.source, pattern.flags.replace("g", "") + "g"));
  if (!matches || matches.length !== 1) {
    console.error(`${path}: expected one match for ${pattern}, found ${matches?.length ?? 0}`);
    process.exit(1);
  }
  return text.replace(pattern, replacement);
}

function edit(path, ...pairs) {
  let text = read(path);
  for (const [pattern, replacement] of pairs) text = replaceOnce(path, text, pattern, replacement);
  write(path, text);
}

edit("Cargo.toml", [/(\[workspace\.package\][^[]*?version\s*=\s*)"[^"]+"/s, `$1"${version}"`]);

// Workspace crates take their version from [workspace.package].
let lock = read("Cargo.lock");
for (const name of ["meetcue-app", "meetcue-audio", "meetcue-core", "meetcue-pipeline", "meetcue-platform"]) {
  lock = replaceOnce("Cargo.lock", lock, new RegExp(`(name = "${name}"\\nversion = )"[^"]+"`), `$1"${version}"`);
}
write("Cargo.lock", lock);

// The first "version" in each package.json is the package's own.
for (const p of ["package.json", "app/ui/package.json", "docs/package.json"]) {
  edit(p, [/^(\s{2}"version":\s*)"[^"]*"/m, `$1"${version}"`]);
}
edit(
  "package-lock.json",
  [/^(\{\n\s{2}"name": "meetcue-workspace",\n\s{2}"version": )"[^"]*"/, `$1"${version}"`],
  [/(\n\s{4}"": \{\n\s{6}"name": "meetcue-workspace",\n\s{6}"version": )"[^"]*"/, `$1"${version}"`],
  [/(\n\s{4}"app\/ui": \{\n\s{6}"name": "meetcue-ui",\n\s{6}"version": )"[^"]*"/, `$1"${version}"`],
  [/(\n\s{4}"docs": \{\n\s{6}"name": "meetcue-docs",\n\s{6}"version": )"[^"]*"/, `$1"${version}"`],
);

if (changelog) {
  const date = new Date().toISOString().slice(0, 10);
  for (const [path, heading] of [
    ["docs/reference/changelog.md", "## Unreleased"],
    ["docs/zh/reference/changelog.md", "## 未发布"],
  ]) {
    const text = read(path);
    if (text.includes(`\n## ${version} — `)) continue;
    const start = text.indexOf(`\n${heading}\n`);
    if (start < 0) {
      console.error(`${path}: no "${heading}" heading`);
      process.exit(1);
    }
    const body = text.slice(start + heading.length + 2).split(/\n## /)[0].trim();
    if (!body) continue;
    write(path, text.replace(`\n${heading}\n`, `\n${heading}\n\n## ${version} — ${date}\n`));
  }
}

if (hash) {
  const url = `https://github.com/Amazingkenneth/meetcue/releases/download/v${version}/meetcue-win-x64.zip`;
  edit(
    "bucket/meetcue.json",
    [/^(\s*"version":\s*)"[^"]*"/m, `$1"${version}"`],
    [/^(\s*"url":\s*)"[^"$]*\/releases\/download\/v[^/"$]+\/meetcue-win-x64\.zip"/m, `$1"${url}"`],
    [/^(\s*"hash":\s*)"[0-9a-fA-F]{64}"/m, `$1"${hash}"`],
  );
}

console.log(`Version set to ${version}`);
