# Providers and keys

This page describes each supported provider: where to get a key, where to enter it, how to check it, and the errors you're most likely to see.

All keys are stored only in Windows Credential Manager, in entries named `MeetCue`. They are never written to the settings file or the logs, and the interface only knows whether a key is configured.

Choose the provider and model for each task in **Settings › Providers & cost › Providers & models**. Clicking the **Model** field opens a list of the provider's models with their price and the estimated cost of a meeting hour. Once a key is saved, the list adds every model your account offers. You can also type any other model name, and each provider has an **Official pricing** link to its own price page.

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

## Alibaba Cloud (Fun-ASR) {#alibaba-cloud}

Alibaba Cloud Model Studio's real-time recognition, built for Mandarin with English terms mixed in and several dialects. Billed per second of audio.

1. **Get a key**: in the Model Studio console, choose the region at the top (Singapore for use outside the Chinese mainland, Beijing inside it) and create an API key there. Keys work only in the region that created them.
2. **Enter it**: **Settings › Providers & cost › Addresses & keys › Alibaba Cloud (Fun-ASR) key**, then **Save and verify**.
3. **Check**: verification starts and ends an empty recognition task, so it uses no audio.
4. **Endpoint**: MeetCue knows both regions' addresses. If the chosen one rejects the key, it tries the other region and keeps using the one that works.
5. **Model**: `fun-asr-realtime` (default) uses your hotwords as context. `paraformer-realtime-v2` is available only in Beijing.

Common errors: [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001) (key rejected in both regions), [MC-ASR-003](/troubleshooting/error-codes#mc-asr-003) (rate limited or account in arrears).

## Tencent Cloud ASR {#tencent-cloud}

Tencent Cloud's real-time recognition, with Mandarin, English, Cantonese and many dialects. Billed per hour of audio.

1. **Get a key**: activate speech recognition in the Tencent Cloud console, then open **Access management › API keys**. You need your **AppID** (shown on the same page and in account info), a **SecretId** and its **SecretKey**.
2. **Enter it**: in **Settings › Providers & cost › Addresses & keys › Tencent Cloud ASR key**, paste all three as `AppID:SecretId:SecretKey`, then **Save and verify**.
3. **Check**: verification opens a signed connection and closes it without sending audio.
4. **Model**: `16k_zh_en` (default) recognises Chinese and English in one sentence plus 31 dialects. `16k_zh` is the cheaper standard Mandarin model, `16k_yue` is Cantonese and `16k_en` is English.
5. **Outside the Chinese mainland**: a Chinese-site account refuses requests from abroad until you turn on cross-border traffic (跨境流量) in the speech recognition console. It needs its own cross-border resource pack or pay-as-you-go billing, and recognition still runs in the mainland, so expect higher latency. Until then MeetCue reports a key error with Tencent's message. An account on the international site avoids this.

Common errors: [MC-ASR-001](/troubleshooting/error-codes#mc-asr-001) (signature rejected, service not activated, or overseas access), [MC-ASR-003](/troubleshooting/error-codes#mc-asr-003) (resource pack used up, arrears or too many streams).

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

## ChatGPT subscription (experimental) {#chatgpt-subscription}

Uses a paid ChatGPT plan that includes Codex (Plus, Pro, Business and others) for question detection and hints instead of an API key. MeetCue signs in the way OpenAI's Codex CLI does and talks to the same backend.

::: warning Not supported by OpenAI
OpenAI supports this sign-in only for its own Codex apps. It may stop working whenever OpenAI changes that backend, and OpenAI could limit or suspend an account that uses it from other apps. Use it at your own risk. The API-key providers above are not affected.
:::

1. **Turn it on**: **Settings › Providers & cost › Advanced › Use my ChatGPT subscription (experimental)**, then confirm the warning.
2. **Choose it**: pick **ChatGPT subscription (experimental)** as the provider for question detection, hints or both.
3. **Sign in**: under **Addresses & keys**, click **Sign in with ChatGPT** and finish in your browser. MeetCue waits for the browser on `127.0.0.1:1455` (or `1457`), so close any Codex sign-in that is still open.
4. **Model**: the list shows the models Codex offers your account. A small one such as `gpt-6-luna` gives the quickest hints.

How it differs from an API key:

- **Speed**: every Codex model reasons before it answers. MeetCue asks for the lowest effort, but the first words still come later than from a fast API model. Run the [latency probe](/guide/latency) to compare.
- **Cost**: requests count toward your plan's usage limits rather than being billed, so no price is shown and budgets don't apply. When the limit is reached you'll see [MC-LLM-003](/troubleshooting/error-codes#mc-llm-003).
- **Tokens**: only the refresh token is kept, in the system credential store. Access tokens stay in memory. **Sign out** deletes the token and asks OpenAI to revoke it.

If the sign-in expires or is revoked, requests fail with [MC-LLM-002](/troubleshooting/error-codes#mc-llm-002): sign in again.

## Corporate networks

MeetCue uses the Windows certificate store, so certificates installed by your company are trusted. It follows the manual proxy set in Windows Settings (**Network & internet › Proxy**) for all connections, including speech recognition. Automatic configuration scripts are not evaluated; if your network uses one, or to use a different proxy, enter it in **Settings › Providers & cost › Advanced › HTTPS proxy**. If connections fail with [MC-NET-001](/troubleshooting/error-codes#mc-net-001), see [High latency](/troubleshooting/latency#company-network).
