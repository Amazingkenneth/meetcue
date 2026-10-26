# Providers and keys

This page describes each supported provider: where to get a key, where to enter it, how to check it, and the errors you're most likely to see.

All keys are stored only in Windows Credential Manager, in entries named `MeetCue`. They are never written to the settings file or the logs, and the interface only knows whether a key is configured.

Choose the provider and type the model name for each task in **Settings › Providers & cost › Providers & models**. MeetCue doesn't list models or prices: once a key is saved, the names the provider reports appear as suggestions, and each provider has an **Official pricing** link to its own price page.

## Soniox

Speech recognition that handles Chinese and English mixed in one sentence, with built-in sentence-end detection. Billed per hour of audio.

1. **Get a key**: sign in to the Soniox console and create an API key.
2. **Enter it**: **Settings › Providers & cost › Addresses & keys › Soniox key**, then **Save and verify**.
3. **Check**: verification lists the real-time models available to your account.
4. **Endpoint**: Soniox runs several endpoints. Keep **Auto** and run the [latency probe](/guide/latency) to choose the fastest.

Common errors: [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001) (key rejected), [MC-ASR-002](/troubleshooting/error-codes#mc-asr-002) (can't connect).

## Deepgram

Streaming speech recognition, billed per minute of audio.

1. **Get a key**: sign in to the Deepgram console, open your project and create an API key with member permissions.
2. **Enter it**: **Settings › Providers & cost › Addresses & keys › Deepgram key**, then **Save and verify**.
3. **Check**: verification lists your projects and the available speech models.
4. **Languages**: set **Settings › Audio › Meeting languages**, main language first. Deepgram can't recognise Chinese and English in the same stream, so with both selected it recognises only the main one; for meetings that mix them, Soniox works better.

Common errors: [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001), [MC-ASR-003](/troubleshooting/error-codes#mc-asr-003) (rate limited).

## Anthropic

Claude models for question detection and hints.

1. **Get a key**: sign in to the Claude Console, open **API keys** and create a key. Add credit or a payment method to the organization.
2. **Enter it**: **Settings › Providers & cost › Addresses & keys › Anthropic key**, then **Save and verify**.
3. **Check**: verification loads the model list from the API; its names appear as suggestions in the **Model** field. You can also type any other model name.
4. **Base URL** (optional): leave it empty to use `https://api.anthropic.com`. To go through a gateway or proxy that speaks the Messages API, enter its address, for example `https://gateway.example.com` (MeetCue adds `/v1` if it's missing).

Common errors: [MC-LLM-002](/troubleshooting/error-codes#mc-llm-002) (key rejected), [MC-LLM-003](/troubleshooting/error-codes#mc-llm-003) (rate limited), [MC-LLM-004](/troubleshooting/error-codes#mc-llm-004) (model not available).

## OpenAI-compatible APIs

Any service that implements the OpenAI chat completions API with streaming, including self-hosted model servers.

1. **Base URL**: choose **OpenAI-compatible** for a task, then in **Settings › Providers & cost › Addresses & keys** enter the base URL ending in `/v1`, for example `https://llm.example.com/v1`.
2. **Key**: paste the service's API key under **OpenAI-compatible key**. Leave it empty if the service doesn't need one.
3. **Check**: **Save and verify** loads `/v1/models`, whose names appear as suggestions in the **Model** field. Type the model name your service uses.
4. **Price**: for cost estimates and budgets, enter the service's price under **Settings › Providers & cost › Budget › Your own prices**.

Some services don't report token usage while streaming. Usage and cost then show as zero for them.

## Corporate networks

MeetCue uses the Windows certificate store, so certificates installed by your company are trusted. It follows the manual proxy set in Windows Settings (**Network & internet › Proxy**) for all connections, including speech recognition. Automatic configuration scripts are not evaluated; if your network uses one, or to use a different proxy, enter it in **Settings › Providers & cost › Advanced › HTTPS proxy**. If connections fail with [MC-NET-001](/troubleshooting/error-codes#mc-net-001), see [High latency](/troubleshooting/latency#company-network).
