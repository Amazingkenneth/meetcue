# Hardware test checklist

This page lists the checks that can only be done on a real Windows computer: audio, shortcuts, the panel, speech synthesis and real provider latency. Automated tests cover the rest. Run through it before each release, after installing with `scoop install meetcue`.

When an item fails, note the error code if any, [export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Self-test first

- [ ] `MeetCue.exe --version` prints the expected version.
- [ ] `MeetCue.exe --self-test`, with speakers muted or headphones on, reports all items as passed.

## Setup and demo

- [ ] The first start opens the setup window. After entering speech and model keys, the latency probe shows a result table and a ranked list of combinations.
- [ ] `MeetCue.exe --demo` plays the whole sample interview.

## Audio

- [ ] Playing a video makes the system sound level move; speaking makes the microphone level move.
- [ ] With speakers instead of headphones, the other side's words are not attributed to you.
- [ ] Unplugging headphones or switching the default device restores capture automatically.

## Meeting

- [ ] In a real meeting, a question block with talking points appears about 1.5 seconds after the other person finishes a question, and the footer latency matches.
- [ ] While answering, points you cover are ticked and the next one is highlighted.
- [ ] A question asked while you answer goes to **Next** without replacing the current hints.
- [ ] All five shortcuts work, and the meeting app's own shortcuts (such as `Ctrl+Shift+M` in Teams) still work.
- [ ] Clicking the panel doesn't take focus from the meeting window.
- [ ] When sharing the whole screen, the other side can't see the panel.

## Practice

- [ ] **Mock interview…** in the tray menu, **Try a mock interview** on the idle panel, **Settings › General › Open mock interview** and **Finish and try a mock interview** in setup all open the mock interview window.
- [ ] Each built-in sample loads with **Use this**; follow the [self-test plan](/guide/mock-interview#self-test-plan) for at least rounds 1 and 3.
- [ ] Practice one round each with the sample subtitles and the sample question list.
- [ ] Hints appear with **Speech-to-text for the questions** both on and off.
- [ ] Switching between the four hint levels works.
- [ ] The review report can be exported.

## Languages and errors

- [ ] Switching between English and Simplified Chinese in the settings changes the panel, tray menu, error messages and practice report immediately.
- [ ] A deliberately wrong key shows an error code; **See the fix** opens the documentation in the current language at that error code.
- [ ] The diagnostics zip contains no API keys.

## Updates and documentation

- [ ] After `scoop update meetcue`, settings, keys, latency history and practice records are still there.
- [ ] The documentation site reads well on a phone and a computer; switching language keeps you on the same page; searching for a Chinese term finds results.
