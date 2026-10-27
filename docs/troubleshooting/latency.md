# High latency

This page helps when hints take noticeably longer than about one and a half seconds, or when requests fail on some networks.

## Hints take more than two or three seconds {#slow-hints}

**Possible causes**

1. The endpoint chosen earlier is no longer the fastest from your current network.
2. The hint model or the detection model is a large, slower model.
3. **Write hints while checking the question** is off, so hints wait for detection to finish.
4. Long briefing material makes every request larger.
5. The sentence-end silence is long.
6. A VPN or proxy adds a long detour.

**Check**

1. Hover over the latency figure in the panel footer: it shows how the last hint's time split between recognition, detection and hints, and the median.
2. When hints are slow several times in a row, the panel names the slowest stage and the one change that helps most, with a button to that setting.
3. Open **Settings › Diagnostics**: each hint shows how long recognition, detection and hint generation took.

**Fix**

1. Keep **Write hints while checking the question** and **Early trigger** on in **Settings › Hints**. Together they save about a second on most questions.
2. Run the [latency probe](/guide/latency) again and apply the fastest combination.
3. Choose a faster hint model, and a small, fast detection model: detection only answers yes or no. The probe's combination table shows the difference.
4. Shorten the briefing material to what this meeting needs.
5. Lower **Sentence end after silence** in **Settings › Audio › Advanced** a little.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## The panel suggests running the probe again {#probe-again}

**Possible causes**

1. Three questions in a row took longer than 2.5 seconds.

**Check**

1. Compare the current latency with your last probe results.

**Fix**

1. Click **Open latency probe** in the notice and run it. Give it a network label so you can compare places later.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Connections fail on a company network {#company-network}

**Possible causes**

1. The network inspects HTTPS traffic with its own certificate ([MC-NET-001](/troubleshooting/error-codes#mc-net-001)).
2. Internet access goes through a proxy that MeetCue doesn't know about.
3. The firewall blocks WebSocket connections, which speech recognition uses.

**Check**

1. Open the provider's website in a browser on the same computer.
2. Ask your IT department whether a proxy is required and whether its certificate is installed in Windows.

**Fix**

1. MeetCue trusts certificates installed in Windows. Ask IT to install the company certificate in the Windows certificate store if it isn't.
2. Enter the proxy in **Settings › Providers & cost › Advanced › HTTPS proxy**, for example `http://proxy.example.com:8080`.
3. Ask IT to allow WebSocket connections to your speech recognition provider's host names shown in the latency probe.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).
