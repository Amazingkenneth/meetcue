# Privacy and data

This page explains what MeetCue sends to which provider, what it keeps on your computer, and what it never collects. MeetCue has no server of its own: everything runs on your computer and talks directly to the providers you chose with your own keys.

## What goes to your speech recognition provider

- Audio from your microphone and from system sound while listening is on, converted to 16 kHz mono. Silence is not sent, except about 0.3 s just before speech starts so the first word isn't cut off, and up to about a second after it so the recognizer can tell the sentence ended.
- Terms from your knowledge base, as recognition hints.
- In practice mode with **Speech-to-text for the questions** off, the other side's audio isn't sent.

## What goes to your language model provider

- For detection: the recognised sentence and a few sentences of context.
- For hints: the question, recent conversation, your role, a short overview of your knowledge base, and the passages of it that match the question.
- In practice mode: questions, your recognised answers, reference answers for feedback, and subtitle lines to classify.
- During the latency probe: a short built-in sample transcript.

## Price lookups

To estimate costs for your usage and budget, MeetCue reads the public pricing pages of Anthropic, Soniox and Deepgram and the [price reference](/reference/pricing) on this site, at start-up. These are ordinary page downloads: no key, account or meeting content is sent.

Each provider's own privacy policy and data retention terms apply to what you send them. Check them before using MeetCue in confidential meetings.

## What stays on your computer

| Data | Location |
| --- | --- |
| API keys | Windows Credential Manager, entries named `MeetCue` |
| Settings, role, custom prompts and extracted knowledge base text | `settings.json` in the settings folder |
| Latency results | `latency-history.json` |
| Usage and cost per call (no content) | `usage.jsonl` |
| Logs, kept 7 days, without transcripts unless debug logging is on (the last lines are also shown in **Settings › Diagnostics › Activity log**) | `logs\` |
| Last price list, for offline use | `pricing.json` |
| Practice reports | `practice\` |

See [Settings](/reference/settings#where-settings-are-stored) for the folder location. Audio is never saved to disk.

## What MeetCue never does

- No telemetry, usage statistics or crash reports are collected.
- API keys are never sent to the interface, written to files or logged.
- Diagnostics exports never include keys, transcripts, knowledge base files, custom prompts or practice records.
- Nothing is sent anywhere while listening is paused, except a latency probe you start yourself.

## Delete your data

**Settings › General › Erase all data** does it in one step: type the confirmation word and MeetCue removes its API keys from Credential Manager, deletes everything in the settings folder listed above, clears the interface's own browser storage, turns off start with Windows, and restarts as if newly installed.

To remove MeetCue entirely instead:

1. Uninstall with `scoop uninstall meetcue --purge` to remove the settings folder.
2. Remove the `MeetCue` entries in **Credential Manager › Windows Credentials**.

Either way, ask your providers to delete data they retained, according to their policies.
