# First-time setup

This page walks through the setup window that opens the first time you start MeetCue. It takes about five minutes, most of it spent creating API keys.

## Before you start

You need an account with one speech recognition provider and one language model provider. MeetCue doesn't pick them for you: the setup window shows prices side by side and you choose.

| Task | Providers in this release |
| --- | --- |
| Speech recognition | Soniox, Deepgram |
| Question detection and hints | Anthropic, any OpenAI-compatible API |

Detection and hints can use different models, and even different providers. Detection is a simple yes/no task, so a cheaper model is usually enough.

## Step 1: Language

Choose **English** or **简体中文**, or let MeetCue follow the Windows display language. You can change it later in **Settings › General**; every window switches immediately.

## Step 2: Audio

MeetCue listens to two sources:

- **Microphone (you)**: the microphone you speak into. *System default* follows the device chosen in Windows Sound settings.
- **Other participants**: everything played through your default output device, which is how meeting apps play other people's voices.

Click **Test levels**, play a video and say something. Both level bars should move. If the system sound bar stays flat, see [Audio and devices](/troubleshooting/audio).

::: tip Use headphones
With speakers, your microphone also picks up the other participants. MeetCue filters most of this echo, but headphones give the cleanest result.
:::

## Step 3: Models and prices

For each of the three tasks, pick a model from the list. Each line shows the model's price (per audio hour, or per million input and output tokens), an estimated cost per hour of meeting, and the measured latency once you have run the latency probe. The list is sorted by estimated cost; **Cheapest** and **Fastest** mark the best figures. You choose each model yourself, and you can't continue until all three are chosen.

Prices are not built into MeetCue. It downloads the [price reference](/reference/pricing), which is checked against each provider's official pricing page, and keeps the last copy for offline use; **Update prices** fetches it again. A model the list doesn't cover links to the provider's official pricing page instead of showing a figure.

## Step 4: API keys

Paste the key for each provider you chose and click **Save and verify**. MeetCue stores keys only in Windows Credential Manager; they never appear in the settings file or the logs.

How to get each key is described in [Providers and keys](/guide/providers):

- [Soniox](/guide/providers#soniox)
- [Deepgram](/guide/providers#deepgram)
- [Anthropic](/guide/providers#anthropic)
- [OpenAI-compatible APIs](/guide/providers#openai-compatible-apis)

If verification fails, the message shows an error code such as [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001). Click **See the fix** to open the matching section.

## Step 5: Latency probe

The probe measures how fast each provider responds from your network and picks the fastest endpoint for each one. It costs a few cents with keys, or nothing if you only measure the network. You can skip it and run it later from **Settings › Providers & cost**. See [Latency probe](/guide/latency).

## Step 6: Done

Press `Ctrl+Alt+Space` when your next meeting starts. To see MeetCue in action first, try the [five-minute tour](/quick-start).
