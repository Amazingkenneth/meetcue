# Latency probe

This page explains how to measure how fast each provider responds from your network, and how MeetCue uses the result to choose endpoints and compare combinations.

## Why measure

The time from the end of a question to the first talking point depends on your network path to each provider, not just on the provider. Many providers offer several endpoints. MeetCue doesn't assume which one suits you: it measures them and shows only the host name and the measured times.

## Run the probe

1. Open **Settings › Providers & cost** and click **Run latency probe**. You can also click the latency figure in the panel footer.
2. Optionally enter a **Network label** such as "home" or "office", so you can compare results from different places later.
3. Without API keys, tick **Network only**. It's free and measures DNS, connection and TLS times for every endpoint.
4. With keys, MeetCue shows the expected cost first: about 30 seconds of audio per speech service and about 8 short model calls, a few cents in total. Click **Run the probe** to confirm.

## What is measured

| Stage | Measurement | Repeats |
| --- | --- | --- |
| Network | DNS lookup, TCP connection and TLS handshake for every endpoint | 5, median |
| Speech recognition | A built-in 9-second recording with Chinese and English, ending in a question, streamed at real-time speed. Start of speech → first partial result, and end of audio → final text | 3 |
| Models | A short transcript with the hint prompt. Request → first token, and request → first complete talking point | 1 warm-up, then 3 |

## Read the results

Each provider row shows the median and the slowest value. The **Combinations** table estimates the end-to-end delay of every speech and model pairing:

> sentence-end wait + recognition + detection + first talking point

and its cost per hour. Sort by fastest, cheapest or balanced, then click **Apply** on the combination you want. Endpoints set to **Auto** switch to the fastest one measured.

## History

Results are saved in `latency-history.json` in your settings folder, with the time and network label. Practice mode uses the most recent results to simulate recognition delay.

## Live monitoring

During a meeting the footer shows the latency of the last hint, and hovering shows the median. If three questions in a row take more than 2.5 seconds, the panel suggests running the probe again; your network or the fastest endpoint may have changed.

::: tip
Only your own measurements count. Latency measured from another network or a cloud server says little about yours.
:::
