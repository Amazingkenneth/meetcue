# Development setup

This page shows how to build MeetCue from source, run the tests and preview the interface without Windows or API keys.

## Requirements

- [Rust](https://rustup.rs) stable. The repository's `rust-toolchain.toml` selects it.
- Node.js 20 or later.
- On Windows: the Microsoft C++ build tools and the WebView2 runtime.
- On Linux, for the desktop app only: the [Tauri system libraries](https://v2.tauri.app/start/prerequisites/) (WebKitGTK, libappindicator and so on).

- On macOS: macOS 13 or later and Xcode Command Line Tools (`xcode-select --install`). Build the distributable app with `npx tauri build` from `app/src-tauri`; use the app bundle for stable privacy permissions.

Clone the repository and install the JavaScript dependencies:

```sh
git clone https://github.com/Amazingkenneth/meetcue
cd meetcue
npm ci
```

## Run the tests

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude meetcue-app
npm run lint
npm run typecheck
npm test
npm run check:locales
npm run check:versions
```

Tests never use the network or real providers. The end-to-end pipeline test replays `samples/transcripts/sdn-interview.jsonl` with mock providers.

Tests that call real providers are marked `#[ignore]` and read keys from environment variables. Run them only on your own computer:

```sh
MEETCUE_ANTHROPIC_KEY=... cargo test -p meetcue-pipeline --test live_providers -- --ignored
```

## Preview the interface in a browser

```sh
npm run dev:mock
```

Open `http://localhost:1420`. The mock backend replays a recorded sample interview through the real components, and the gallery switches between the panel, settings, first-run setup and practice views. This works on any operating system.

## Run the desktop app

```sh
npx tauri dev
```

Run it from `app/src-tauri`. Useful flags for the built program:

| Flag | Effect |
| --- | --- |
| `--demo` | Replays the sample interview with mock providers |
| `--self-test` | Checks audio, shortcuts, screen-capture hiding and providers, prints a report and exits |
| `--version` | Prints the version |

`MEETCUE_CONFIG_DIR` points the app at a different settings folder, and `MEETCUE_LOG` sets the log filter.

## Work on the documentation

```sh
npm run docs:dev
npm run docs:build
npm run docs:lint
```

`docs:build` also generates the error code and price pages and checks that every English page has a Chinese page with the same number of sections.

## Generated files

TypeScript types for settings, events and command results are generated from the Rust types with ts-rs when the core tests run. After changing a Rust type that the interface uses, run `cargo test -p meetcue-core` and commit the changed files in `app/ui/src/bindings`. CI fails if they are out of date.
