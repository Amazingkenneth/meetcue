# First-time setup

This page walks through the setup window that opens the first time you start MeetCue. It takes about five minutes, most of it spent creating API keys.

## Before you start

You need an account with one speech recognition provider and one language model provider. MeetCue doesn't pick them for you: you choose the provider and type the model name for each task.

| Task | Providers in this release |
| --- | --- |
| Speech recognition | Soniox, Deepgram, Alibaba Cloud (Fun-ASR), Tencent Cloud ASR |
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

## Step 3: Providers and models

For each of the three tasks, choose a **Provider** and type a **Model** name. Any name the provider accepts works, including models released after this version of MeetCue; once the provider's key is saved, the names it reports appear as suggestions while you type. You can't continue until all three tasks have a provider and a model.

MeetCue doesn't list models or prices. Each provider has an **Official pricing** link to its own price page; for OpenAI-compatible services, check the pricing page of the service you use.

Below the tasks, **Addresses & keys** shows a row for each provider you chose:

- **Base URL**: required for OpenAI-compatible services, for example `https://llm.example.com/v1`. For Anthropic it's optional: leave it empty to use `https://api.anthropic.com`, or enter a gateway or proxy that speaks the same API.
- **Key**: paste it and click **Save and verify**. MeetCue stores keys only in Windows Credential Manager; they never appear in the settings file or the logs.

How to get each key is described in [Providers and keys](/guide/providers):

- [Soniox](/guide/providers#soniox)
- [Deepgram](/guide/providers#deepgram)
- [Alibaba Cloud (Fun-ASR)](/guide/providers#alibaba-cloud)
- [Tencent Cloud ASR](/guide/providers#tencent-cloud)
- [Anthropic](/guide/providers#anthropic)
- [OpenAI-compatible APIs](/guide/providers#openai-compatible-apis)

If verification fails, the message shows an error code such as [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001). Click **See the fix** to open the matching section.

## Step 4: Latency probe

The probe measures how fast each provider responds from your network and picks the fastest endpoint for each one. It costs a few cents with keys, or nothing if you only measure the network. You can skip it and run it later from **Settings › Providers & cost**. See [Latency probe](/guide/latency).

## Step 5: Done

Press `Ctrl+Alt+Space` when your next meeting starts. To see MeetCue in action first, try the [five-minute tour](/quick-start).

To test everything with your own voice before a real meeting, click **Finish and try a mock interview**. See [Mock interview](/guide/mock-interview).
