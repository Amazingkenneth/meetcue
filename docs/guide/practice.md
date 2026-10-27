# Practice mode

This page shows how to rehearse an interview or a meeting with MeetCue. It reads questions aloud, gives you hints exactly as in a real meeting, and gives you a review report afterwards.

::: tip First time?
[Mock interview](/guide/mock-interview) walks through a complete practice run step by step, with built-in sample interviews in English and Chinese.
:::

## Import questions

Open **Mock interview…** from the tray menu, or click **Try a mock interview** on the panel. Pick a built-in sample interview, or choose your own file. Files are processed on your computer; only the text of questions and your answers is sent to the model.

| Type | Formats | Notes |
| --- | --- | --- |
| Meeting subtitles | `.srt`, `.vtt`, `.txt` | Speakers come from a `Name:` prefix or a VTT `<v Name>` tag. Choose which speaker is you. |
| Question list | `.txt`, `.md`, `.csv` | One question per line; a line starting with `#` starts a group. CSV columns are described in [File formats](/reference/file-formats). |

Subtitle files without speaker names are labelled question or answer by the model; you check the result before starting.

After import, the preview table lets you edit, delete and reorder questions, shuffle them, or keep only a number of them.

The built-in samples are also in the repository under [samples/practice](https://github.com/Amazingkenneth/meetcue/tree/HEAD/samples/practice), with matching job descriptions under [samples/materials](https://github.com/Amazingkenneth/meetcue/tree/HEAD/samples/materials).

## The asking voice

Questions are read by a Windows voice. In the mock interview window's first step, choose a voice for English and one for Chinese, press **Try it** to hear it, and set the reading speed. **First voice Windows offers** keeps MeetCue's own choice. Install more voices in Windows Settings › Time & language › Speech.

## Speech-to-text

Questions are always spoken aloud with the Windows speech voices, as in a real meeting. Two switches decide what is transcribed:

- **Speech-to-text for the questions** (on by default): the spoken question is captured from system sound and recognised by your speech provider like a real meeting. The delay you see is the delay you'll get in a meeting. When off, the question's text goes straight to the hint model after a recognition delay drawn from your last latency probe. This saves the speech recognition cost for the other side. The model's own delay is real.
- **Speech-to-text for my answers** (on by default): your microphone is recognised for coverage ticks and the review. Turning it off as well removes all speech recognition cost, but also the ticks and some review figures.

## Pace and follow-ups

- For a question list, MeetCue asks a question and waits. If you don't start within 20 seconds, the panel shows a gentle reminder. A pause of 3 seconds, or `Ctrl+Alt+Enter`, ends your answer.
- With follow-ups on, MeetCue sometimes (30% by default) asks a short follow-up question based on your answer.
- For subtitles, other speakers' lines play on the original timeline, with long gaps shortened to 2 seconds. Playback stops at your lines and waits for you; the original line becomes the reference answer in the review.
- `Ctrl+Alt+]` skips a question; `Ctrl+Alt+Space` pauses and resumes. **Ask again**, in the practice window and on the panel, reads the current question from the start and discards the answer you had begun.
- If you start speaking while a question is still being read, playback stops. Only real speech counts: a cough, a filler word or a word misheard from the question itself doesn't cut it short. **Stop reading when I start speaking** turns this off.

## Hint levels

Choose a level before you start or change it during practice:

1. **Full**: the same as meeting mode.
2. **Keywords only**: the bold keywords, without details or the sample answer.
3. **Delayed**: hints appear a few seconds after the question ends.
4. **Off**: hints only appear in the review.

Step down one level at a time until you can answer without hints.

## Review report {#review-report}

When you finish, the review shows for each question and for the whole session:

| Figure | Meaning |
| --- | --- |
| Time to start | From the end of the question to your first recognised word |
| Answer length | From your first word to the end of your answer |
| Coverage | Talking points ticked off out of all points |
| Filler words | "um", "uh", "like", "you know", and 嗯、啊、那个、就是、然后 for Chinese; editable in settings |
| Pace | Words per minute (characters per minute for Chinese) |
| Feedback | With a reference answer, three short notes: what you missed, what to phrase better, what went well |

Export the report as Markdown. Past sessions are kept in the `practice` folder next to your settings, and the review shows your trend over time.
