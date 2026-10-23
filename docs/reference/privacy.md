# Privacy and data

This page explains what MeetCue sends to which provider, what it keeps on your computer, and what it never collects. MeetCue has no server of its own: everything runs on your computer and talks directly to the providers you chose with your own keys.

## What goes to your speech recognition provider

- Audio from your microphone and from system sound while listening is on, converted to 16 kHz mono. Silence is not sent.
- Terms from your briefing material, as recognition hints.
- In practice mode with **Simulated latency**, the other side's audio isn't sent.

## What goes to your language model provider

- For detection: the recognised sentence and a few sentences of context.
- For hints: the question, recent conversation, your role, and a short excerpt of your briefing material.
- In practice mode: questions, your recognised answers, reference answers for feedback, and subtitle lines to classify.
- During the latency probe: a short built-in sample transcript.

Each provider's own privacy policy and data retention terms apply to what you send them. Check them before using MeetCue in confidential meetings.

## What stays on your computer

| Data | Location |
| --- | --- |
| API keys | Windows Credential Manager, entries named `MeetCue` |
| Settings, role and extracted briefing text | `settings.json` in the settings folder |
| Latency results | `latency-history.json` |
| Usage and cost per call (no content) | `usage.jsonl` |
| Logs, kept 7 days, without transcripts unless debug logging is on | `logs\` |
| Practice reports | `practice\` |

See [Settings](/reference/settings#where-settings-are-stored) for the folder location. Audio is never saved to disk.

## What MeetCue never does

- No telemetry, usage statistics or crash reports are collected.
- API keys are never sent to the interface, written to files or logged.
- Diagnostics exports never include keys, transcripts, briefing material or practice records.
- Nothing is sent anywhere while listening is paused, except a latency probe you start yourself.

## Delete your data

1. Uninstall with `scoop uninstall meetcue --purge` to remove the settings folder.
2. Remove the `MeetCue` entries in **Credential Manager › Windows Credentials**.
3. Ask your providers to delete data they retained, according to their policies.
