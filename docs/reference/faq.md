# FAQ

Short answers to common questions. For problems, start at [Troubleshooting](/troubleshooting/).

## Does MeetCue need a subscription?

No. MeetCue is free and open source under the MIT license. You pay the speech and model providers directly, with your own keys, at their usual prices. The settings show an estimated cost per hour before you choose, and the actual usage afterwards.

## How much does an hour of meetings cost?

It depends on the models you choose, and the difference can be dozens of times. A typical setup with a small model costs a few tens of US cents per hour, mostly for speech recognition. The setup window and **Settings › Providers & cost** show an estimate for your choices; see also the [price reference](/reference/pricing).

## Can the other participants see MeetCue?

The panel is hidden from screen sharing and most recordings by default. Some recording methods and capture hardware can still see it; see [screen sharing](/troubleshooting/panel#screen-share). MeetCue doesn't join the meeting or send anything to the meeting app.

## Is it fair to use MeetCue in an interview?

That's for you and the situation to decide. Many interviews and exams don't allow outside help; follow the rules you agreed to. Practice mode is designed for preparing without any of these concerns.

## Which languages are supported?

The interface is in English and Simplified Chinese. Recognition and hints work in Chinese and English, including both in one sentence. Other languages may work with providers that support them, but aren't tested.

## Does it work on macOS or Linux?

The app builds on both, but audio capture, the panel behaviour and installation packages are only finished for Windows in this release.

## Can I use a model running on my own computer?

Yes, if it offers an OpenAI-compatible API with streaming. Enter its base URL under [OpenAI-compatible APIs](/guide/providers#openai-compatible-apis). Speech recognition still needs one of the supported cloud services.

## Why does MeetCue need my microphone?

To know when you start and stop answering, to queue questions while you speak, and to tick off talking points you've covered. Turn off **Track what I've said** if you only want hints.
