# Briefing material

This page shows how to give MeetCue background about you and the meeting, so the hints match what you would actually say.

## Describe your role

In **Settings › Hints › My role**, write one sentence about yourself, for example:

> Network architect with 8 years in data center operations

The hint model writes talking points from this perspective.

## Add files

1. Open **Settings › Hints**.
2. Under **Briefing material**, click **Add a file…**.
3. Choose an agenda, résumé, project notes or similar. Supported formats are `.txt`, `.md`, `.docx` and `.pdf`.

MeetCue extracts the text on your computer and shows how many characters were found. Scanned PDFs without a text layer yield no text; export them with text recognition first.

## What is sent

- A short excerpt of the material is included in the hint model's instructions, so hints can refer to your projects and the agenda.
- The most distinctive terms (product names, acronyms, people's names) are sent to the speech recognition provider as hints, so they are recognised correctly.
- The full text stays on your computer. It is never included in diagnostics exports.

The material is placed in the part of the instructions that stays the same during a meeting. Providers that support prompt caching then charge much less for it after the first question.

To try it with a mock interview, use one of the sample job descriptions: [jd-software-engineer.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-software-engineer.md) or [jd-security-ai.md](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/materials/jd-security-ai.md). They match the built-in sample interviews in [Mock interview](/guide/mock-interview).

## Keep it focused

Long material makes every hint request larger and slower to start. One or two pages that match the meeting work better than a full document archive. Remove files you no longer need with **Remove**.
