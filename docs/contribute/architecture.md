# Architecture

This page gives an overview of how MeetCue is put together, so you know where to look before changing something.

## Overview

MeetCue is a Tauri 2 desktop app. All real-time work runs in Rust; the React interface only displays events and sends commands. This matters because a hidden or minimised WebView slows down its timers, which would make recognition and hints stutter.

```text
 microphone ─┐                      ┌─> speech recognition (WebSocket)
             ├─> audio ─> pipeline ─┤
 system sound┘   (16 kHz)  engine   └─> language model (HTTPS, streaming)
                             │
                             └─> events ─> panel and settings (React)
```

## Repository layout

| Path | Contents |
| --- | --- |
| `crates/core` | Shared types: settings, events, error codes (`data/errors.json`), price table (`data/pricing.json`) |
| `crates/pipeline` | The hint engine, provider clients, latency probe, practice logic, prompts (`prompts/*.txt`) |
| `crates/audio` | Audio capture (WASAPI on Windows), format conversion, voice activity detection |
| `crates/platform` | Credential store, settings paths, speech synthesis, window styles |
| `app/src-tauri` | The desktop app: windows, tray, shortcuts, commands, sessions, diagnostics |
| `app/ui` | React interface, translations (`locales/`), browser preview backend (`src/mock/`) |
| `docs` | This documentation site |
| `samples` | Test data written for this project |

## The pipeline engine

`crates/pipeline/src/engine.rs` is a synchronous state machine: it takes an input (a transcript piece, a model chunk, a timer tick, a user action) and the current time, and returns the events and requests that follow. It has no I/O, so tests can drive it step by step with a fake clock.

`backend.rs` runs the engine: it owns the provider streams, applies the requests (start, cancel or retry a hint), and forwards events to the app. Key rules enforced here:

- Every hint generation carries a version; cancelled or outdated versions are dropped and their HTTP streams closed.
- At most two generations run at the same time; more wait in a queue.
- Detection only looks at the other participants' channel.
- Transcript history is trimmed to the last ten minutes.

## Providers

Provider traits are in `provider.rs`; implementations are in `providers/`. Each client handles its own protocol details (keep-alive, finalisation, streaming format) and reports usage exactly as the provider returns it. Money is counted in integer micro-dollars.

Endpoints are listed in `crates/pipeline/data/endpoints.json` and chosen by the latency probe. Nothing in the code prefers one endpoint over another.

## The app and the interface

- Commands are listed once in `app/src-tauri/src/command_names.rs`. The build script, the capability file and the TypeScript command map are checked against this list by tests, so a missing permission fails CI instead of failing at run time.
- Events go out on two channels: `meetcue://event` for the pipeline and `meetcue://app` for app state.
- API keys are read from the credential store in Rust and never cross into the interface.

## Error codes

Every user-facing error has a code from `crates/core/data/errors.json` with a title and a fix in both languages. The interface reads the same file, and `docs/scripts/gen-errors.mjs` builds the error code page from it, so app and documentation can't drift apart.
