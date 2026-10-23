# Export diagnostics

This page shows how to collect the information needed to look into a problem, and what that file contains.

## Export the file

1. Right-click the MeetCue tray icon and choose **Export diagnostics…**, or open **Settings › Diagnostics** and click **Export…**.
2. Click **Show what will be included** to see the file list.
3. Choose where to save the zip file.

## What is included

| Item | Details |
| --- | --- |
| Logs | MeetCue's log files from the last three days. API keys and tokens are masked. |
| Settings | `settings.json` without briefing material text. Keys are never in this file. |
| Latency history | `latency-history.json` from the latency probe. |
| System information | Windows version, MeetCue version, audio device names and sample rates. |

## What is never included

- API keys and access tokens.
- Transcripts of what was said. Logs don't contain transcripts unless you turned on debug logging, and even then they aren't exported.
- Briefing material and practice records.

## Run the self-test

`MeetCue.exe --self-test` checks the parts that are hard to test anywhere but on your computer and prints a short report:

- system sound capture receives a test tone (mute the speakers or wear headphones);
- the microphone level;
- all shortcuts register;
- the panel is hidden from screenshots;
- providers respond, if keys are configured.

Copy the report into your issue.

## Report a problem

[Open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml). The form asks for the error code, your MeetCue version (`MeetCue.exe --version`) and the steps that lead to the problem. Attach the diagnostics zip.

## Debug logging

If asked, turn on **Settings › Diagnostics › Debug logging**, reproduce the problem and export again. Debug logging also writes transcripts to the log on your computer. Turn it off afterwards.
