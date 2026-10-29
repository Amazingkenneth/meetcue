# Changelog

All notable changes to MeetCue. Versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

## 0.3.2 — 2026-10-29

- macOS 13+ native microphone and system-audio capture, permission guidance, transparent overlays, and app/DMG builds for Apple Silicon and Intel.
- Reuse the macOS speech engine so listing voices does not break subsequent voice previews and mock interviews; reopen audio streams after device failures or sleep.

## 0.3.0 — 2026-10-27

### Added

- The mock interview offers four built-in samples, two in English and two in Chinese: software engineer, network security and AI, backend software engineer (Chinese) and telecom network operations (Chinese, 42 questions in seven groups).
- **Go to a question** jumps to any question during a mock interview, forwards or back, from the practice window or the panel's practice bar.
- Batch changes before a mock interview: tick several questions (Shift-click for a range) to move them to the top or bottom, keep only them, set their group or delete them. The list can be filtered by text or group, and you can add your own questions.
- When Windows has no voice for English or Chinese, the sound check says so and links to the Windows settings that install one, with a step-by-step guide.
- **Move** on the panel opens one-click placement for touchpads: snap to any edge or corner (clear of the taskbar), move to the next screen, narrow/normal/wide and ⅓, ½ or full height, opacity, text size, hide for 6 seconds, and let clicks pass through.
- **Saved spots**: save up to three panel positions, each with its size and layout, and jump to them with `Ctrl+Alt+Shift+1`, `2` and `3`. On an empty spot the shortcut saves where the panel is now.
- `Ctrl+Alt+Shift+P` hides the panel for 6 seconds so you can click what is behind it; `Ctrl+Alt+Shift+T` and a tray menu item turn click-through on and off.
- **Font** (**Settings › General**) changes the font in every window, and the panel's text size and opacity can be set there too.

### Fixed

- A double tap and drag on a touchpad, or a double click on the title bar, blew the panel up to cover the whole screen. The panel can now be dragged from any empty part of it and never maximizes.
- A size you drag the panel's edges to is kept instead of being reset on the next layout change.
- Recording a shortcut with `Shift` and a digit stored the symbol (`!`) instead of the digit.

## 0.2.4 — 2026-10-27

### Added

- **Boost quiet voices from the meeting** (**Settings › Audio**, on by default) raises soft-spoken participants to a normal speaking level before recognition, and system sound has its own, lower voice threshold (−60 dBFS), so quiet questions are no longer missed.
- **Write hints while checking the question** (**Settings › Hints**, on by default) starts hint generation together with question detection, so the first point usually appears about a second sooner.
- Hovering over the latency in the panel footer shows how the last hint's time split between recognition, detection and hints. When hints are slow, the panel names the slowest stage and the setting that helps most.

### Fixed

- `MC-SYS-002` ("settings folder not writable") appeared for failures that had nothing to do with the settings folder, such as a window or file dialog that couldn't open. Those now report `MC-SYS-006`, and a failed settings save reports `MC-SYS-005`.
- The check whether the settings folder next to the program is writable could fail for a moment after sign-in or an update and move settings to `%APPDATA%\MeetCue`. It now retries, and the notice about it is shown once instead of on every start.

## 0.2.3 — 2026-10-26

### Added

- The mock interview window is a guided four-step flow: a sound check with live level bars, choosing the questions, reviewing them, and practising. Every step says what comes next, and the running step says what is expected of you right now.
- **Ask again** reads the current question from the start, in the practice window and on the panel.
- The voice that reads the questions can be chosen per language in the sound check step, with **Try it** to hear it. Reading speed moved there too.

### Fixed

- A question could stop halfway through: a cough, an "um" or a single word misheard from the question's own echo counted as the user answering. Interrupting now needs real speech, and filler words from the practice settings never count.

## 0.2.2 — 2026-10-26

### Added

- Mock interviews are easier to start: **Mock interview…** is near the top of the tray menu, the idle panel links to it, and it can be opened from **Settings › General** and from the last step of setup. The window starts with a step-by-step **How it works**.
- Practice playback is now a **Speech-to-text for the questions** switch: on (the default) transcribes the spoken questions exactly like a meeting; off sends their text straight to the hint model. Questions are spoken aloud either way. **Recognise my answers** is now **Speech-to-text for my answers**.
- Built-in sample interviews for software engineering and for network security with AI, as question lists and as interview transcripts, load with one click. Sample job descriptions to use as briefing material are in `samples/materials`.
- A [mock interview guide](/guide/mock-interview) explains how to test the whole app over your microphone without a real meeting.

## 0.2.1 — 2026-10-26

### Fixed

- The level meters stopped moving as soon as a meeting or practice session started, in the panel and in **Settings › Audio**, so the microphone looked dead while MeetCue was listening. Practice also missed when you started speaking.

## 0.2.0 — 2026-10-26

### Changed

- Providers and models are entered freely, the same way in setup and in **Settings › Providers & cost**: choose a provider and type any model name for each task. The names a provider reports appear as suggestions; MeetCue no longer lists models or prices, and links to each provider's official pricing page instead.
- Anthropic can be reached through a gateway or proxy: enter its base URL next to the key. Base URLs are now stored per provider in `providers.baseUrls`; an existing OpenAI-compatible base URL is kept.
- Prices for budget tracking are entered under **Settings › Providers & cost › Budget › Your own prices**.

### Fixed

- Opening **Run setup again**, the activity log or any other window from a button froze MeetCue on Windows: no window could be moved and the app couldn't be quit.
- Speech recognition:
  - System sound recognition stopped about 20 seconds into a meeting when nothing was playing yet, showing `MC-ASR-002`. The connection is now kept alive whether or not audio arrives, and a stream the provider ends is reopened without waiting for sound.
  - An unreachable or slow provider held up everything else: stopping or quitting could take up to a minute while MeetCue retried, and audio was dropped meanwhile. Connecting now happens in the background, and every network step has a time limit.
  - Error messages now show what went wrong (for example `connection timed out`), connection errors disappear once recognition is back, and the panel marks a side whose recognition is reconnecting.
  - A setting the provider rejects is reported as `MC-ASR-004` instead of a network problem (`MC-ASR-002`), and isn't retried many times a second.
  - Connections no longer time out on networks where IPv6 is configured but doesn't work.
- The panel's level meters stayed flat until a session started, so the microphone looked dead in the panel while it worked in **Settings › Audio**. The visible panel now shows both levels before you press **Start**; nothing is sent anywhere until you do.
- A microphone that delivers only silence (blocked in Windows privacy settings, or muted) is now reported with `MC-AUD-002`.
- Unexpected errors inside MeetCue are now written to the log.

## 0.1.3 — 2026-10-24

### Changed

- Prices are read live from Anthropic's, Soniox's and Deepgram's official pricing pages, and from OpenAI-compatible servers that list prices with their models. Every current Claude model now shows its price and estimated cost; only a model with no price anywhere links to the pricing page.
- Deepgram shows the rate billed today, including limited-time promotions, with the regular rate in the tooltip.
- Changing a model, key, language or hint setting during a meeting takes effect right away: the next question uses the new models, and recognition switches without losing the sentence in progress.
- Every setting can now be changed in the app: sliders for the voice threshold and end-of-sentence silence (now up to 3 s), panel layout and **Reset panel position** in **General**, **Run setup again**, all cost estimate assumptions, a list of your own prices with **Remove**, and the filler words counted in practice reviews. A new microphone or voice threshold also applies during a meeting.

### Added

- **Settings › Diagnostics › Activity log** shows what MeetCue is doing as it happens: starting and pausing, whether audio arrives from each side, recognised sentences, questions and errors. Also in the tray menu, and from any error message.

### Fixed

- Clicking **Start** or **Resume** when something was missing (a key, a model, the audio device) did nothing; the reason is now shown, and a session whose audio fails to start is no longer left half running.
- Notices such as a shortcut that's already taken were never shown.
- The meeting clock kept counting while paused.
- The settings tab list scrolled away on long pages.
- Speech recognition:
  - The first sound of a sentence was often cut off; the 0.3 s before speech is now sent with it.
  - Recognition stopped for the rest of the meeting after a second connection drop, and after pausing for more than a few seconds; it now keeps the connection alive while paused and reconnects whenever needed.
  - With Deepgram, English sentences lost the spaces between their parts ("yourexperience").
  - With Deepgram and both Chinese and English selected, English was sent to the Mandarin model. The first language in **Meeting languages** is now the main one, and there is a "Mostly English, some Chinese" choice.
  - The end-of-sentence silence setting was cut short at 0.8 s.

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
