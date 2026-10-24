# Changelog

All notable changes to MeetCue. Versions follow [Semantic Versioning](https://semver.org/).

## 0.1.1 — 2026-10-24

### Fixed

- Deepgram, Anthropic and OpenAI-compatible keys failed verification and sessions using them would not start: only Soniox was wired up.
- The setup window turned blank instead of closing when you clicked **Finish**.
- Switches in Settings were drawn with the knob off-centre.

### Removed

- Amazon Bedrock. Use Anthropic or an OpenAI-compatible API for hints.

## 0.1.0 — 2026-10-23

### Added

- Meeting mode: live question detection with early trigger, numbered talking points in about a second and a half, a question queue, and coverage ticks.
- Floating panel that doesn't take focus, with full and mini layouts, click-through and hiding from screen sharing.
- Speech recognition with Soniox and Deepgram; hints with Anthropic, Amazon Bedrock and OpenAI-compatible APIs.
- Latency probe with endpoint selection, cost estimates and combination ranking.
- Price table, usage tracking, budgets and CSV export.
- Practice mode: subtitle and question-list import with preview editing, questions read aloud with system voices, real or simulated recognition, follow-up questions, four hint levels, and a review report with Markdown export.
- Briefing material from txt, md, docx and pdf files.
- English and Simplified Chinese interface and documentation.
- Error codes with linked fixes, diagnostics export, `--demo` and `--self-test`.
