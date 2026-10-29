# Five-minute tour

This page shows MeetCue working end to end with a recorded sample interview, so you can learn the panel before a real meeting. No API keys or microphone are needed.

## Start the demo

1. Close MeetCue if it's running (right-click the tray icon › **Quit MeetCue**).
2. Press `Win+R`, type the command below and press Enter:

   ```text
   MeetCue.exe --demo
   ```

   If Windows can't find it, use the full path. With Scoop it's `%USERPROFILE%\scoop\apps\meetcue\current\MeetCue.exe`.

3. The panel appears on the right side of the screen with a **Demo** tag, and the replay starts after a moment. `Ctrl+Alt+Space` pauses and resumes it.

## What you'll see

The demo replays an interview for a network engineering role. Watch for these parts of the panel:

1. **Question card**: about a second and a half after the interviewer finishes a question, a card appears with the question in grey and numbered talking points below it.
2. **Talking points**: each point has a bold keyword and a few grey words of detail. As the candidate covers a point, it gets a green tick and dims. The next point to cover is highlighted with a **Next** tag.
3. **Other questions**: when the interviewer cuts in while the candidate is answering, the new question waits as a one-line row marked **Waiting** below the current one. The current hints are not replaced. Click any row to show that question in full.
4. **Footer**: the latency of the most recent hint. Click it to open the latency probe.

## Try the shortcuts

While the demo runs, try these shortcuts. They work even though another app has focus.

| Shortcut | Action |
| --- | --- |
| `Ctrl+Alt+Space` | Start or pause listening |
| `Ctrl+Alt+H` | Panel: full → mini bar → hidden |
| `Ctrl+Alt+[` / `Ctrl+Alt+]` | Previous / next question |
| `Ctrl+Alt+Enter` | Mark the current question answered |

## Next steps

- Set up your providers in [First-time setup](/setup).
- Read [Meeting mode](/guide/meeting) for how questions are detected and queued.
- Test everything over your microphone, without a meeting, in a [mock interview](/guide/mock-interview).
- Rehearse with your own questions in [Practice mode](/guide/practice).
