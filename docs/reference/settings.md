# Settings

This page lists every setting, where to find it in the app, and its key in `settings.json`. You normally change settings in the app; editing the file by hand is only needed for automation or recovery.

## Where settings are stored

| Install | Settings folder |
| --- | --- |
| Scoop | `scoop\persist\meetcue\config` (kept across updates) |
| Portable zip | `config` next to `MeetCue.exe` |
| Fallback when that folder isn't writable | `%APPDATA%\MeetCue` |

The folder holds `settings.json`, `latency-history.json`, `usage.jsonl`, `logs\` and `practice\`. **Settings › Diagnostics › About** shows the folder in use. API keys are never stored here; they live in Windows Credential Manager.

If `settings.json` can't be read, MeetCue renames it to `settings.json.bad` and starts with defaults.

## General

| Setting | Key | Default |
| --- | --- | --- |
| Interface language | `general.uiLanguage`: `system`, `en`, `zh-CN` | `system` |
| Hint language | `general.hintLanguage`: `followQuestion`, `en`, `zh-CN` | `followQuestion` |
| Accent color | `general.accent` | `#F5B841` |
| Start MeetCue when I sign in | `general.autostart` | `false` |
| Hide the panel when sharing the screen | `overlay.hideInScreenShare` | `true` |
| Let clicks pass through the panel | `overlay.clickThrough` | `false` |
| Panel layout | `overlay.mode`: `full`, `mini`, `hidden` | `full` |
| Panel position (drag the panel; **Reset panel position** puts it back) | `overlay.x`, `overlay.y` | top right |
| Run setup again | `general.wizardCompleted` | `false` until setup is finished |

Every setting applies as soon as it's changed, including during a meeting: models, keys, languages and hint settings take effect for the next question, and recognition and audio capture switch without restarting the session.

## Audio

| Setting | Key | Default |
| --- | --- | --- |
| Microphone (you) | `audio.micDevice`; empty uses the system default | empty |
| Other participants | `audio.remoteSource`: `allSystemSound` | `allSystemSound` |
| Meeting languages | `audio.languages`, e.g. `["zh", "en"]` | `["zh", "en"]` |
| Voice detection threshold | `audio.vadThresholdDb`, in dBFS, −70 to −10 | `-45` |
| Sentence end after silence | `audio.endpointSilenceMs`, 500 to 3000 ms | `700` |

## Providers and cost

| Setting | Key |
| --- | --- |
| Speech recognition | `providers.asr` |
| Question detection model | `providers.detect` |
| Hint model | `providers.hints` |
| Base URL per provider (required for OpenAI-compatible, optional for Anthropic) | `providers.baseUrls`, for example `{"openai-compatible": "https://host/v1"}`. Older files with `providers.openaiBaseUrl` are moved here when loaded |
| HTTPS proxy | `providers.proxy`; empty uses the system proxy |
| Prices you entered (listed under **Your own prices**, where they can be removed) | `providers.pricingOverrides` |
| Cost estimate assumptions (**How estimates are calculated**) | `providers.estimate`: `speechRatio`, `questionsPerHour`, `hintInputTokens`, `hintOutputTokens`, `detectInputTokens`, `detectOutputTokens`, `systemTokens`, `cancelOverhead` |
| Budget | `providers.budget.sessionLimitUsd`, `monthlyLimitUsd`, `onLimit` (`notifyOnly`, `pauseHints`, `endSession`) |

Each of `asr`, `detect` and `hints` has `provider`, `model` and `endpoint`. The endpoint is `{"mode": "auto"}` (fastest measured) or `{"mode": "host", "host": "<host name>"}`.

## Hints

| Setting | Key | Default |
| --- | --- | --- |
| My role | `hints.role` | empty |
| Briefing material | `hints.materials` (name, path and extracted text) | none |
| Only questions directed at me | `hints.onlyDirectedToMe` | `false` |
| Queue interruptions | `hints.queueInterruptions` | `true` |
| Track what I've said | `hints.trackCoverage` | `true` |
| Early trigger | `hints.earlyTrigger` | `true` |

## Shortcuts

| Setting | Key | Default |
| --- | --- | --- |
| Start or pause listening | `hotkeys.toggleCapture` | `Ctrl+Alt+Space` |
| Cycle the panel | `hotkeys.cyclePanel` | `Ctrl+Alt+H` |
| Previous question | `hotkeys.focusPrev` | `Ctrl+Alt+[` |
| Next question | `hotkeys.focusNext` | `Ctrl+Alt+]` |
| Mark answered | `hotkeys.markAnswered` | `Ctrl+Alt+Enter` |
| Move the panel up | `hotkeys.moveUp` | `Ctrl+Alt+Shift+Up` |
| Move the panel down | `hotkeys.moveDown` | `Ctrl+Alt+Shift+Down` |
| Move the panel left | `hotkeys.moveLeft` | `Ctrl+Alt+Shift+Left` |
| Move the panel right | `hotkeys.moveRight` | `Ctrl+Alt+Shift+Right` |

## Practice

| Setting | Key | Default |
| --- | --- | --- |
| Speech-to-text for the questions | `practice.playback`: `realPipeline` (on), `simulatedLatency` (off) | `realPipeline` |
| Hint level | `practice.hintLevel`: `full`, `headsOnly`, `delayed`, `off` | `full` |
| Delay for delayed hints (s) | `practice.hintDelayS` | `5` |
| Pause that ends an answer (s) | `practice.answerPauseS` | `3` |
| Reminder when you haven't started (s) | `practice.nudgeAfterS` | `20` |
| Follow-up probability | `practice.followUpProbability` | `0.3` |
| Speech rate | `practice.speechRate` | `1.0` |
| Speech-to-text for my answers | `practice.recognizeMe` | `true` |
| Stop reading when I start speaking | `audio.stopTtsOnSpeech` | `true` |
| Filler words | `practice.fillersZh`, `practice.fillersEn` | see [Practice mode](/guide/practice#review-report) |

## Diagnostics

| Setting | Key | Default |
| --- | --- | --- |
| Debug logging | `diagnostics.debugLogging` | `false` |
