# File formats

This page describes the files practice mode can import, with examples. All files can be UTF-8 (with or without BOM), UTF-16 or a legacy encoding such as GBK; MeetCue detects the encoding. Windows and Unix line endings both work.

## Subtitles: SRT

Standard SubRip files. Put the speaker's name at the start of the text, followed by a colon.

```text
1
00:00:01,200 --> 00:00:04,800
Interviewer: Could you start by introducing yourself?

2
00:00:05,500 --> 00:00:12,000
Alex: Sure. I've worked on data center networks for eight years.
```

Timestamps may use a comma or a dot before the milliseconds.

## Subtitles: WebVTT

Speakers come from `<v Name>` voice tags or a `Name:` prefix. Hours can be left out.

```text
WEBVTT

00:01.200 --> 00:04.800
<v Interviewer>Could you start by introducing yourself?

00:05.500 --> 00:12.000
<v Alex>Sure. I've worked on data center networks for eight years.
```

## Subtitles: plain text

One sentence per line, optionally with a `Name:` prefix and an optional timestamp in brackets. Full-width colons (`：`) work too.

```text
[00:01] Interviewer: Could you start by introducing yourself?
[00:05] Alex: Sure. I've worked on data center networks for eight years.
```

Without timestamps, MeetCue plays the lines one after another. Without speaker names, the model labels each line as a question or an answer, and you check the result.

## Question list: text and Markdown

One question per line. A line starting with `#` starts a group; the group name appears in the preview and the review. Empty lines are ignored, and Markdown list markers (`-`, `*`, `1.`) are removed.

```markdown
# Basics
- What is the difference between the control plane and the data plane?
- How does BGP choose a best path?

# Design
- How would you design a network for two data centers?
```

## Question list: CSV

The first row names the columns. Only `question` is required.

| Column | Meaning |
| --- | --- |
| `question` | The question |
| `reference_answer` | A model answer, used for feedback in the review |
| `category` | A group name |
| `followups` | Follow-up questions, separated by `\|` |

```text
question,reference_answer,category,followups
"What is SDN?","Separates control and data planes; central controller; programmable through APIs.",Basics,"Which controllers have you used?|What are the risks?"
```

## Sample files

- [sdn-interview.srt](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/sdn-interview.srt): a short interview in Chinese and English.
- [questions-network.csv](https://github.com/Amazingkenneth/meetcue/blob/HEAD/samples/practice/questions-network.csv): ten network architecture questions with reference answers.
