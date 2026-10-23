# Practice mode

This page shows how to rehearse an interview or a meeting with MeetCue. It reads questions aloud, gives you hints exactly as in a real meeting, and gives you a review report afterwards.

## Import questions

Open **Practice…** from the tray menu and choose a file. Files are processed on your computer; only the text of questions and your answers is sent to the model.

| Type | Formats | Notes |
| --- | --- | --- |
| Meeting subtitles | `.srt`, `.vtt`, `.txt` | Speakers come from a `Name:` prefix or a VTT `<v Name>` tag. Choose which speaker is you. |
| Question list | `.txt`, `.md`, `.csv` | One question per line; a line starting with `#` starts a group. CSV columns are described in [File formats](/reference/file-formats). |

Subtitle files without speaker names are labelled question or answer by the model; you check the result before starting.

After import, the preview table lets you edit, delete and reorder questions, shuffle them, or keep only a number of them.

Try it with the sample files: [sdn-interview.srt](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/sdn-interview.srt) and [questions-network.csv](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/questions-network.csv).

## Choose how questions are played

- **Real pipeline** (default): questions are spoken through your speakers with the Windows speech voices and recognised by your speech provider like a real meeting. The delay you see is the delay you'll get in a meeting.
- **Simulated latency**: questions are still spoken, but their text goes straight to the hint model after a recognition delay drawn from your last latency probe. This saves the speech recognition cost for the other side. The model's own delay is real.

Your microphone is still recognised in both modes for coverage ticks and the review. You can turn that off too, which removes all speech recognition cost but also the ticks and some review figures.

## Pace and follow-ups

- For a question list, MeetCue asks a question and waits. If you don't start within 20 seconds, the panel shows a gentle reminder. A pause of 3 seconds, or `Ctrl+Alt+Enter`, ends your answer.
- With follow-ups on, MeetCue sometimes (30% by default) asks a short follow-up question based on your answer.
- For subtitles, other speakers' lines play on the original timeline, with long gaps shortened to 2 seconds. Playback stops at your lines and waits for you; the original line becomes the reference answer in the review.
- `Ctrl+Alt+]` skips a question; `Ctrl+Alt+Space` pauses and resumes. If you start speaking while a question is still being read, playback stops.

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
