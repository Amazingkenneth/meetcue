# Meeting mode

This page explains how MeetCue follows a live meeting: when it shows hints, how it handles several questions in a row, and how to read the panel.

## Start and pause listening

Press `Ctrl+Alt+Space`, or click **Start** on the panel. The status dot turns green and the timer starts. Press the shortcut again to pause; nothing is sent to any provider while paused.

You can also start and pause from the tray icon menu.

## How a question becomes hints

1. **Recognition**: both audio sources are streamed to your speech recognition provider. Only sound that contains speech is sent; silence is skipped, apart from a short keep-alive signal.
2. **Detection**: when another participant finishes a sentence, the detection model decides whether it's a question for you. Your own microphone is never checked for questions.
3. **Hints**: for a question, the hint model writes three to five numbered talking points and a short sample answer. Points appear one by one as they are generated.

With **Early trigger** on (**Settings › Hints**), MeetCue starts generating as soon as the speaker pauses for a moment, before the sentence is officially over. If the speaker keeps going and the question changes noticeably, the early request is cancelled and a new one starts. This saves several hundred milliseconds on most questions.

With **Write hints while checking the question** on (also in **Settings › Hints**, on by default), detection and hint generation start at the same time instead of one after the other. The hints are shown only if detection confirms a question, so nothing changes on the panel, but the first point usually arrives about a second sooner. A sentence that looks like a question but isn't costs one unused hint request.

## Read the panel

From top to bottom, the full panel shows:

1. **Title bar**: status, elapsed time, pause, keep on top, and the switch to the mini bar.
2. **Sources**: a level meter for your microphone and one for system sound. They move as soon as the panel is shown, before you start, so you can check that MeetCue hears you. Nothing is sent to any provider until you start.
3. **Question queue**: the previous, current and next question, with their status. Click a card to scroll to that question.
4. **Conversation**: one block per question. Click the grey question text to see the sentences around it and why it was detected. **Show full answer** expands the sample answer.
5. **Footer**: shortcut reminders and the latency of the last hint.

The **mini bar** (`Ctrl+Alt+H`) shows only the current question, its progress, the next point to cover in large type, and how many questions are waiting.

To keep your eyes near the camera, move the panel right under it with `Ctrl+Alt+Shift` and the arrow keys. See [Move the panel](/guide/shortcuts#move-the-panel).

## Coverage ticks

With **Track what I've said** on, MeetCue compares what you say with the talking points. A point you've covered gets a green tick and dims, and the next uncovered point is marked **Next**. Matching uses words for English and character pairs for Chinese, so paraphrasing a point closely is enough.

If you use speakers instead of headphones, your microphone also hears the other participants. MeetCue ignores microphone text that closely matches what was just played, so their words don't tick off your points.

## Several questions at once

- **Queue interruptions** (on by default): a question asked while you are answering waits as **Next**. When you finish answering, it becomes the current question.
- A question asked while you are not speaking replaces the current one, which moves to **Previous**.
- Use `Ctrl+Alt+[` and `Ctrl+Alt+]` to move between questions, and `Ctrl+Alt+Enter` to mark the current one answered.

A question counts as answered only after you have spoken and then stopped, so silence before your answer never archives it.

## Hide the panel from screen sharing

**Hide the panel when sharing the screen** is on by default. Teams, Zoom, browser screen sharing and most recording tools then capture the area behind the panel instead of the panel itself.

::: warning Not a guarantee
Some recording methods and external capture hardware can still capture the panel. Test with your meeting app before relying on it.
:::

## Cost during a meeting

Each call's usage is recorded and converted with the downloaded price list; calls to models without a price are counted but not costed. **Settings › Providers & cost** shows this month's usage by provider. You can set a per-meeting and a monthly budget; MeetCue warns at 80% and, at 100%, only notifies you, pauses hints or ends the session, as you choose.
