# Changelog

All notable changes to MeetCue. Versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

### Fixed

- A question no longer loses its first words when the interviewer pauses right after them. Recognition ends a sentence at every pause, so "上任后。" / "一年的计划是什么？" could become a card for "一年的计划是什么？" titled "一年的计划". The detection model now also sees how long each pause was, which is how it tells a sentence cut by a breath from two separate ones, and it is asked to keep such a first half with the question and in its title.
- When the rest of a question arrives after a pause and joins its card, the detection model now writes a new title and list of parts for the whole question. Before, the title kept describing only the first part. The hints still restart at once and don't wait for the title.

## 0.4.0 — 2026-11-06

### Added

- **Meeting recaps.** When you stop listening, MeetCue saves the questions, their hints and which points you ticked, and the panel offers **View the recap**. **Settings › Meetings** lists saved meetings to read, save as Markdown or delete. The recap costs nothing; the full transcript is saved only if you turn that on. On by default, kept 90 days. See [After the meeting](/guide/meeting#recap).
- **Remember my earlier answers** (**Settings › Hints**, on): the hint model gets one line per earlier question with what you answered, so a follow-up late in a long meeting stays consistent with what you said. See [Earlier answers stay in mind](/guide/meeting#meeting-memory).
- **Shorter**, **More detail** and **Another take** under a question's hints, and **Ask** in the title bar to type a question the recognizer missed (**Settings › General › Buttons to rewrite hints**, on). See [Rewrite hints or type a question](/guide/meeting#hint-actions).
- **Write questions for my role** in the mock interview: practice questions with reference answers written from your role and knowledge base, in one request. See [Questions for your own role](/guide/mock-interview#generate).
- **Tell speakers apart** (**Settings › Audio**, off): question cards show **Speaker 1**, **Speaker 2** with Soniox or Deepgram. Off by default because Deepgram charges extra for it.
- The panel says plainly when speech recognition stays disconnected for 10 seconds or hints fail twice in a row (**Settings › General**, on).
- **Settings › Providers & cost** shows this month's cost by purpose: recognition, detection, hints and the rest.

### Changed

- **Don't check repeated sentences** (**Settings › Hints**, on): a sentence that repeats one checked in the last two minutes, such as a question asked again, is no longer sent to the detection model a second time.
- The text extracted from knowledge base files is kept in `knowledge.json` and written only when it changes, so `settings.json` stays small. Older settings files are moved over on the next save.

## 0.3.11 — 2026-11-05

### Fixed

- A question the interviewer splits over several pauses now stays on one card. Recognition ends a sentence at every pause, often with a full stop, so before this "如何制定数据中心IP。" / "V6的迁移方案，同时保证。" / "存量用户不受影响？" could turn into three questions, and a short tail such as "调整？" was dropped. Sentences that follow without anyone answering are now joined, and the hints are started for the whole question at once.
- The start of a new question is no longer added to the previous question after you have answered it.
- Sentences the interviewer uses to set a question up ("如果研发和运维团队的职责重叠……我们发现了这些问题，那你会如何调整？") are now shown on the question card with it.

### Changed

- Talking points are written in plain spoken sentences you can read out as they are, instead of a short phrase and a list of keywords.
- The detection reply is used as soon as it is complete, without waiting for the rest of the response, which shaves a little off the time to the first hint.

## 0.3.10 — 2026-11-03

### Changed

- Whether a sentence needs an answer is now decided by the detection model from its meaning, not by question words or a question mark. Requests such as "说一下你对微服务的理解" or "I'd like to hear about your last role" now get hints. Every sentence from the other side, apart from very short back-channels, is now sent to the detection model, so detection makes more requests than before.

## 0.3.9 — 2026-11-03

### Fixed

- When the interviewer pauses mid-question, the rest of the sentence is now added to the question card instead of showing up as a separate question or being dropped. Two questions asked close together also no longer swap places in the list when the reply for the later one comes back first.

## 0.3.8 — 2026-11-02

### Changed

- Hints for open-ended questions, such as "how do you balance work and family?" or "tell me about a time when…", are no longer a few generic lines. They now give a clear stance, two to four concrete practices, an example and a trade-off, with a longer sample answer that is still easy to say out loud. Short factual questions keep the short answer.
- The panel now follows the meeting: the newest question is always shown in full at the top, even while it waits for you to finish the previous one, so its hints are never only in the list below. The question list is in the order questions were asked, newest first, and earlier questions move down as new ones come in. Clicking a line shows that question in the same place above the list, and **Back to the latest question** or the next new question brings you back.

## 0.3.7 — 2026-11-01

### Changed

- Hints start faster. MeetCue now loads the hint instructions into Claude's prompt cache when a session starts and keeps them there while people are talking, so the first question of a meeting no longer waits for the whole prompt to be read. The output language moved from the instructions to the question message, so one cache entry serves questions in every language. If you edited the hint prompts, `{output_language}` now belongs in the question message; old instructions that still use it keep working.
- **Settings › Providers & cost › Usage** shows the share of input tokens served from the prompt cache for each model. Cache hits reported by DeepSeek and Kimi are now counted and priced as such.
- The hourly cost estimate no longer assumes detection is served from the cache, and counts the cost of writing the hint prompt to it.

## 0.3.6 — 2026-11-01

### Changed

- In a long meeting the panel's list of other questions stays short: only the three most recent earlier questions are shown, older ones fold behind **Show earlier questions**, and **Clear earlier** hides them from the list until you restore them.
- Ticking off talking points from your microphone is now off by default, also for existing installs, because it could tick or miss points when your wording differed from the list. Click a point to tick it, or turn the setting back on under **Settings › Hints › Tick off points as I speak**.
- The tray menu is shorter and keeps only what you need during a meeting: start or pause, show or hide the panel, click-through, mock interview, settings, help and quit. **Show or hide the panel** now does exactly that in one click and brings back the layout you last used, instead of stepping through full, mini and hidden. **Reset panel position** is in **Settings › General**, and the activity log and diagnostics export are in **Settings › Diagnostics**. **Settings › General** also has a link to the help pages.

### Fixed

- Hints keep coming in long meetings. A hint reply that stops half way now times out after 15 seconds and keeps the points it already got, so stalled replies no longer leave later questions stuck on **Generating hints…**. A question still waiting when a new one arrives more than two minutes later is set aside, and the session history keeps recording for the whole meeting.
- Word knowledge base files with tables, text boxes, charts, SmartArt and pictures now import cleanly: tables keep one line per row, chart and SmartArt text and picture alt text are included, and layout numbers, duplicated text boxes and deleted text no longer leak in. Files that can't be read say why, such as password protection, an old .doc file renamed to .docx, or a damaged file.

## 0.3.5 — 2026-10-31

### Changed

- The **Model** field opens a list of the provider's models with each one's price and the estimated cost of a meeting hour, the cheapest marked. It works before a key is saved; once one is, the list adds every model your account offers. Type to filter, or enter any other model name.

## 0.3.4 — 2026-10-31

### Added

- Two speech recognition providers for Mandarin meetings: **Alibaba Cloud (Fun-ASR)**, with Singapore and Beijing regions, and **Tencent Cloud ASR**, with Chinese-English and dialect models. See [Providers and keys](/guide/providers#alibaba-cloud).

## 0.3.3 — 2026-10-29

### Added

- **Knowledge base**: add your résumé, project notes, slides or spreadsheets, and hints build on your real experience. Long files are searched per question on your computer, so each question gets the passages that match it. New formats: Excel, PowerPoint (with speaker notes), OpenDocument, RTF and HTML, and several files can be added at once.
- **Try a question** shows which knowledge base passages a question would get.
- **Settings › Hints › Prompts** shows the hint model's instructions and lets you edit them, with warnings for missing placeholders and a button to restore the built-in text.

### Changed

- A question the speaker pauses in the middle of ("your thoughts on idea A… and idea B") stays one question: the rest is joined to the same card and the hints are rewritten to cover every part, instead of answering only the first half. This also applies when the early trigger fired at the pause.
- Requests that don't sound like questions, such as "Talk about…", "Share a time…" or "谈谈你的看法", are now recognised.
- Compound questions keep the hints written while detection was still checking them, so their first point arrives about a model round trip sooner.
- Hints lead with a direct answer, cover every part of a compound question in order, read through misheard words, and the sample answer is shorter and easier to say out loud.
- The panel shows only the current question in full, with every other question as one line below it. Click a line to look at that question, and **Back to the current question** returns in one click.
- A question asked before you started answering the previous one becomes the current question instead of waiting as the next one.

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
