# Audio and devices

This page helps when MeetCue doesn't hear you or the other participants, or hears the wrong thing. Open **Settings › Audio › Test levels** to watch both level bars while you work through the steps.

## The system sound level stays flat {#no-system-sound}

**Possible causes**

1. The meeting app plays through a different device than the Windows default output.
2. Nothing is playing, or the meeting app is muted.
3. Another program holds the output device in exclusive mode ([MC-AUD-001](/troubleshooting/error-codes#mc-aud-001)).
4. The audio driver doesn't support loopback capture ([MC-AUD-005](/troubleshooting/error-codes#mc-aud-005)).

**Check**

1. Open **Settings › System › Sound** in Windows and note the output device.
2. In your meeting app's audio settings, check which speaker it uses.
3. Play a video in a browser and watch the system sound bar.

**Fix**

1. Set the meeting app's speaker to **Default** or to the same device as the Windows output.
2. If MeetCue reports MC-AUD-001: open **Sound › More sound settings**, open the device's **Properties › Advanced**, clear **Allow applications to take exclusive control of this device**, and restart the program that held the device.
3. Update the audio driver from your computer manufacturer.

::: tip Test with headphones
When you test with speakers, the microphone picks up the sound and its bar moves. That doesn't mean system sound capture works. Mute the speakers or use headphones and watch the system sound bar only. `MeetCue.exe --self-test` checks exactly this.
:::

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## The microphone level stays flat {#no-microphone}

**Possible causes**

1. Windows blocks microphone access for desktop apps ([MC-AUD-002](/troubleshooting/error-codes#mc-aud-002)).
2. MeetCue uses a different microphone from the one you speak into.
3. The microphone is muted by a hardware key or in the meeting app's driver.

**Check**

1. Open **Settings › Privacy & security › Microphone** in Windows.
2. In MeetCue, open **Settings › Audio** and look at **Microphone (you)**.

**Fix**

1. Turn on **Microphone access** and **Let desktop apps access your microphone**.
2. Choose your microphone in **Microphone (you)**, or choose **System default** and set the default in Windows.
3. Press the laptop's microphone mute key if its light is on.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Capture stops after changing headphones or speakers {#device-changed}

**Possible causes**

1. The default device changed and capture is restarting ([MC-AUD-004](/troubleshooting/error-codes#mc-aud-004)).
2. The new device is in use exclusively by another program.

**Check**

1. Look for the "The audio device changed; capture restarted" notice in the panel.
2. Watch the level bars for a few seconds after the change.

**Fix**

1. Wait two or three seconds; MeetCue rebuilds capture on its own.
2. If the bars stay flat, pause and resume with `Ctrl+Alt+Space`.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## The other side's words tick off my points {#echo}

**Possible causes**

1. You use speakers, so the microphone picks up the other participants.
2. The microphone is very sensitive or close to the speakers.

**Check**

1. Look at the conversation: lines marked **Me** that repeat what someone else just said.

**Fix**

1. Use headphones. This is the most reliable fix.
2. Turn the speaker volume down, or move the microphone away from the speakers.
3. Raise **Settings › Audio › Advanced › Voice detection threshold** a little.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Playback sounds worse with a Bluetooth headset {#bluetooth}

**Possible causes**

1. When an app opens a Bluetooth headset's microphone, Windows switches the headset to its call mode, which has lower sound quality for everything.

**Check**

1. **Settings › Audio** shows a warning when the selected microphone looks like a Bluetooth device.

**Fix**

1. Choose the computer's built-in microphone in **Microphone (you)** and keep the headset for listening.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).
