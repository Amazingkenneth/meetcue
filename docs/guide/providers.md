# Providers and keys

This page describes each supported provider: where to get a key, where to enter it, how to check it, and the errors you're most likely to see.

All keys are stored only in Windows Credential Manager, in entries named `MeetCue`. They are never written to the settings file or the logs, and the interface only knows whether a key is configured.

Prices are listed in the [price reference](/reference/pricing). Check the provider's website for current prices.

## Soniox

Speech recognition that handles Chinese and English mixed in one sentence, with built-in sentence-end detection. Billed per hour of audio.

1. **Get a key**: sign in to the Soniox console and create an API key.
2. **Enter it**: **Settings › Providers & cost › API keys › Soniox key**, then **Save and verify**.
3. **Check**: verification lists the real-time models available to your account.
4. **Endpoint**: Soniox runs several endpoints. Keep **Auto** and run the [latency probe](/guide/latency) to choose the fastest.

Common errors: [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001) (key rejected), [MC-ASR-002](/troubleshooting/error-codes#mc-asr-002) (can't connect).

## Deepgram

Streaming speech recognition, billed per minute of audio.

1. **Get a key**: sign in to the Deepgram console, open your project and create an API key with member permissions.
2. **Enter it**: **Settings › Providers & cost › API keys › Deepgram key**, then **Save and verify**.
3. **Check**: verification lists your projects and the available speech models.
4. **Languages**: set **Settings › Audio › Meeting languages**. Deepgram uses its Mandarin model whenever Chinese is selected and its multilingual model otherwise.

Common errors: [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001), [MC-ASR-003](/troubleshooting/error-codes#mc-asr-003) (rate limited).

## Anthropic

Claude models for question detection and hints.

1. **Get a key**: sign in to the Claude Console, open **API keys** and create a key. Add credit or a payment method to the organization.
2. **Enter it**: **Settings › Providers & cost › API keys › Anthropic key**, then **Save and verify**.
3. **Check**: verification loads the model list from the API. Models in the price table show their prices; others show **Price unknown**.

Common errors: [MC-LLM-002](/troubleshooting/error-codes#mc-llm-002) (key rejected), [MC-LLM-003](/troubleshooting/error-codes#mc-llm-003) (rate limited), [MC-LLM-004](/troubleshooting/error-codes#mc-llm-004) (model not available).

## OpenAI-compatible APIs

Any service that implements the OpenAI chat completions API with streaming, including self-hosted model servers.

1. **Base URL**: in **Settings › Providers & cost**, enter the base URL ending in `/v1`, for example `https://llm.example.com/v1`.
2. **Key**: paste the service's API key under **OpenAI-compatible key**. Leave it empty if the service doesn't need one.
3. **Check**: **Save and verify** loads `/v1/models`. Prices are unknown for these models; enter them with **Set a price** so cost estimates and budgets work.

Some services don't report token usage while streaming. Usage and cost then show as zero for them.

## Corporate networks

MeetCue uses the Windows certificate store, so certificates installed by your company are trusted. It follows the manual proxy set in Windows Settings (**Network & internet › Proxy**) for all connections, including speech recognition. Automatic configuration scripts are not evaluated; if your network uses one, or to use a different proxy, enter it in **Settings › Providers & cost › Advanced › HTTPS proxy**. If connections fail with [MC-NET-001](/troubleshooting/error-codes#mc-net-001), see [High latency](/troubleshooting/latency#company-network).
