# Changelog

All notable changes to MeetCue. Versions follow [Semantic Versioning](https://semver.org/).

## 0.1.3 — 2026-10-24

### Changed

- Prices are read live from Anthropic's, Soniox's and Deepgram's official pricing pages, and from OpenAI-compatible servers that list prices with their models. Every current Claude model now shows its price and estimated cost; only a model with no price anywhere links to the pricing page.
- Deepgram shows the rate billed today, including limited-time promotions, with the regular rate in the tooltip.

### Added

- **Settings › Diagnostics › Activity log** shows what MeetCue is doing as it happens: starting and pausing, whether audio arrives from each side, recognised sentences, questions and errors. Also in the tray menu, and from any error message.

### Fixed

- Clicking **Start** or **Resume** when something was missing (a key, a model, the audio device) did nothing; the reason is now shown, and a session whose audio fails to start is no longer left half running.
- Notices such as a shortcut that's already taken were never shown.
- The meeting clock kept counting while paused.
- The settings tab list scrolled away on long pages.

## 0.1.2 — 2026-10-24

### Changed

- Prices are no longer built into the app. MeetCue downloads the [price reference](/reference/pricing), checked against each provider's official pricing page, keeps the last copy for offline use, and can update it from **Settings › Providers & cost**.
- A more compact **Models & prices** layout: one line per model with its price and estimated cost per hour, sorted by cost, with **Cheapest** and **Fastest** marked. API keys take one row per provider.
- A model without a published price links to the provider's official pricing page instead of showing **Price unknown**.

### Fixed

- Deepgram Nova-3 used the multilingual rate. MeetCue requests one language for Chinese or English, which costs $0.0077 per minute.

### Added

- Prices for Claude Sonnet 5, Claude Opus 5 and Claude Fable 5.1.

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
