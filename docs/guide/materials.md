# Knowledge base

This page shows how to give MeetCue background about you and the meeting, so the hints build on your real experience instead of generic answers, and how to see and change the instructions the hint model gets.

## Describe your role

In **Settings › Hints › My role**, write one sentence about yourself, for example:

> Network architect with 8 years in data center operations

The hint model writes talking points from this perspective.

## Add files

1. Open **Settings › Hints**.
2. Under **Knowledge base**, click **Add files…**. You can pick several files at once.
3. Choose a résumé, project notes, a slide deck, a job description, an agenda or similar.

Supported formats:

| Kind | Extensions |
| --- | --- |
| PDF | `.pdf` |
| Word | `.docx`, `.docm` |
| Excel | `.xlsx`, `.xlsm` (every sheet, as rows of cells) |
| PowerPoint | `.pptx`, `.pptm` (slide text and speaker notes) |
| OpenDocument | `.odt`, `.ods`, `.odp` |
| Other documents | `.rtf`, `.html`, `.htm` |
| Text | `.txt`, `.md`, `.csv`, `.tsv`, `.json`, `.yaml`, `.log` |

MeetCue extracts the text on your computer and shows how many characters were found. Old `.doc`, `.xls` and `.ppt` files can't be read: open them in Office and save them as `.docx`, `.xlsx` or `.pptx` first. Scanned PDFs without a text layer yield no text; export them with text recognition first. Adding a file again replaces the earlier copy.

## How the knowledge base is used

- **Short files** (up to about 3,000 characters in total) are sent whole with every question.
- **Longer files** are split into passages. For each question, MeetCue picks the passages that share the most words with it, up to about 2,400 characters, and adds them to that question's request. A short overview of every file is also included. The search runs on your computer, in well under a millisecond, so it doesn't slow the hints down.

The hint model is told to build its answer on the experience these notes describe, and to use your real projects, names and figures instead of inventing them.

To check what a question would get, type it under **Try a question** and click **Show matches**. Nothing is sent anywhere.

## What is sent

- The overview, and the passages that match each question, go to the hint model.
- The most distinctive terms (product names, acronyms, people's names) are sent to the speech recognition provider as hints, so they are recognised correctly.
- The full text stays on your computer. It is never included in diagnostics exports.

The overview is placed in the part of the instructions that stays the same during a meeting. Providers that support prompt caching then charge much less for it after the first question.

To try it with a mock interview, use one of the sample job descriptions: [jd-software-engineer.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-software-engineer.md) or [jd-security-ai.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-security-ai.md). They match the built-in sample interviews in [Mock interview](/guide/mock-interview).

## Change the prompts

**Settings › Hints › Prompts** shows the two prompts the hint model gets, and lets you edit them:

- **Instructions** are the same for every question. Placeholders: `{role}` (your role), `{materials}` (the knowledge base overview), `{output_language}`.
- **Question message** is sent with each question. Placeholders: `{knowledge}` (the matching passages), `{transcript}` (the recent conversation), `{question}`.

A placeholder you remove means that information no longer reaches the model; MeetCue warns you when one is missing. Keep the reply format described in the instructions (numbered JSON lines, a line with `---`, then the sample answer), or the panel can't show the hints.

Edited prompts are marked **Edited**. **Restore built-in** goes back to the built-in text, which is updated with new versions of MeetCue. Changes apply to the next question, even during a meeting.

## Keep it focused

The passages are picked by shared words, so files that use the same terms as the questions work best: write project notes in the language of the meeting, and name the technologies you used. Remove files you no longer need with **Remove**.
