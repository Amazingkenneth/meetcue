# Release process

Releases are automatic. Every push to the default branch that changes the app is released once CI passes on it; nobody bumps the version or pushes a tag by hand.

## Before merging

1. Add the change to the **Unreleased** section of both [changelogs](/reference/changelog). The release moves these entries under the new version and uses them as the release notes.
2. Check prices in `crates/core/data/pricing.json` against the providers' pricing pages and update the `verified` dates and `updated` when they change. The app downloads this file from the documentation site, so a price fix only needs a push to the default branch, not a release.
3. For changes to audio, recognition or windows, run the [hardware test checklist](/test-checklist) on a Windows computer with a build from the branch (CI uploads `MeetCue-windows-x64` for every push).

## Choosing the version

The version is the latest `v*` tag with its patch number raised. Put a marker in the pull request title, or in the title of a commit pushed straight to the default branch, to change that. A marker that only appears in a description is ignored, so describing the markers never triggers them.

| Marker | Effect |
| --- | --- |
| `[release minor]` | 0.1.5 → 0.2.0 |
| `[release major]` | 0.1.5 → 1.0.0 |
| `[skip release]` | in the newest commit: don't release this push |

Pushes that only touch documentation, workflows or the Scoop manifest are not released. To release by hand, run the **Release** workflow from the Actions tab and choose which part to raise.

## What the release workflow does

`.github/workflows/release.yml` runs when CI finishes on the default branch:

1. Decides the version. Only the newest commit is released: if the branch moved on while CI ran, the newer commit's run releases both.
2. Builds `MeetCue.exe` on Windows with that version written into the manifests, checks `MeetCue.exe --version`, and packs `MeetCue.exe`, `LICENSE` and the README files into `meetcue-win-x64.zip`.
3. Runs `scripts/set-version.mjs`, which writes the version into `Cargo.toml`, `Cargo.lock`, the `package.json` files and `package-lock.json`, moves the **Unreleased** changelog entries under the new version, and points `bucket/meetcue.json` at the new zip and its SHA-256 hash.
4. Commits this as "Release x.y.z" to the default branch, as the identity in the repository variables `GIT_NAME` and `GIT_EMAIL`, and tags it `vx.y.z`. The commit and tag are pushed together; if the branch moved in the meantime, nothing is published and the next run releases instead.
5. Creates the GitHub release with the zip and the changelog section as notes, and republishes the documentation site.
6. Installs Scoop on a Windows runner, installs MeetCue from the tagged manifest and checks that it prints the new version.

`npm run check:versions` checks that every manifest, both lockfiles and the Scoop manifest carry the same version, so the committed version always equals the latest release. The hash is only ever computed and committed by the workflow, in the same run that uploads the zip. Never replace a release asset by hand; publish a new version instead.

If a run fails after the commit and tag were pushed, use **Re-run failed jobs**: it finishes the release for the same tag.

## After the release

1. Check that `scoop update meetcue` picks up the new version on a Windows computer.
2. Check that the documentation site shows the new changelog.

## Repository settings

These are set once:

- **Settings › Pages**: source is the `gh-pages` branch. The docs workflow publishes there on every change to `docs/` or `errors.json` on the default branch.
- **Settings › Secrets and variables › Actions › Variables**: `GIT_NAME` and `GIT_EMAIL` for commits made by workflows.
- **Settings › Actions › General › Workflow permissions**: the workflow pushes to the default branch with `GITHUB_TOKEN`, so a branch protection rule on it must let GitHub Actions push.
- The repository must be public, or Scoop can't download release assets.

Mac releases are built on Apple Silicon and Intel runners as `.app` ZIPs and DMGs. The bundle check verifies privacy descriptions, the macOS 13 minimum, signature integrity and `--version`. Packages currently use ad-hoc signing; Developer ID signing and notarization require Apple credentials and are not configured by this change.
