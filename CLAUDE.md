# MeetCue for coding agents

MeetCue is a Tauri 2 desktop app: Rust crates under `crates/`, the app under `app/src-tauri`, a React interface under `app/ui` and a VitePress site under `docs`. `docs/contribute/architecture.md` explains where things live.

## Before you push

Run what CI runs and fix anything that fails:

```sh
mkdir -p app/ui/dist && [ -f app/ui/dist/index.html ] || echo '<!doctype html>' > app/ui/dist/index.html
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude meetcue-app
npm ci
npm run lint && npm run typecheck && npm test
node scripts/check-locales.mjs
```

Tests regenerate the TypeScript bindings in `app/ui/src/bindings` and `app/ui/src/mock/default-settings.json`; commit those changes too.

## House rules

- Interface text goes in both `app/ui/locales/en.json` and `zh-CN.json`. Documentation changes go in both `docs/` and `docs/zh/`.
- Never commit API keys, tokens, real meeting recordings or transcripts.
- Don't bump versions or edit `bucket/`: releases set the version from the tag.
- Keep a change to the one issue you were given. Write commit messages as one plain sentence saying what changed.

## Agent pull requests

Work started from an issue labelled `agent` lands on a `claude/` branch, gets a pull request opened for it, and is merged automatically once CI passes. Changes to workflows, release files, versions, dependencies or this file always wait for a human review instead (see `.github/agent-protected-paths.txt`). If CI fails, you get up to three attempts to fix it.
