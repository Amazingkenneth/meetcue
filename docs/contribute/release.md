# Release process

This page describes how a MeetCue release is made: from bumping the version to the updated Scoop manifest.

## Prepare

1. Make sure CI is green on the default branch.
2. Check prices in `crates/core/data/pricing.json` against the providers' pricing pages and update the `verified` dates and `updated`. The app downloads this file from the documentation site, so a price fix only needs a push to the default branch, not a release.
3. Run the [hardware test checklist](/test-checklist) on a Windows computer with a build from the default branch.
4. Move the **Unreleased** entries in both [changelogs](/reference/changelog) under the new version heading.

## Bump the version

The version lives only in the root `Cargo.toml` under `[workspace.package]`. Also update `package.json`, `app/ui/package.json` and `docs/package.json` to the same value; `npm run check:versions` checks that they agree. `tauri.conf.json` takes the version from Cargo.

## Tag

```sh
git tag v0.2.0
git push origin v0.2.0
```

## What the release workflow does

Pushing a `v*` tag runs `.github/workflows/release.yml` on Windows:

1. Builds `MeetCue.exe` with the Tauri CLI, without an installer.
2. Packs `MeetCue.exe`, `LICENSE` and the README files into `meetcue-win-x64.zip` and computes its SHA-256 hash.
3. Creates the GitHub release with the zip and generated notes.
4. Updates `bucket/meetcue.json` with the new version, URL and hash, and commits it to the default branch as the identity in the repository variables `GIT_NAME` and `GIT_EMAIL`.
5. Installs Scoop on the runner, installs MeetCue from the updated manifest and checks that `MeetCue.exe --version` prints the new version.

The hash is only ever computed and committed by the workflow, in the same run that uploads the zip. Never replace a release asset by hand; publish a new version instead.

## After the release

1. Check that `scoop update meetcue` picks up the new version on a Windows computer.
2. Check that the documentation site shows the new changelog.

## Repository settings

These are set once:

- **Settings › Pages**: source is the `gh-pages` branch. The docs workflow publishes there on every change to `docs/` or `errors.json` on the default branch.
- **Settings › Secrets and variables › Actions › Variables**: `GIT_NAME` and `GIT_EMAIL` for commits made by workflows.
- The repository must be public, or Scoop can't download release assets.
