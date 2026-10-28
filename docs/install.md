# Install MeetCue

This page shows how to install, update and remove MeetCue on Windows and macOS. The whole process works without administrator rights.

## Requirements

- Windows 10 version 2004 or later, or Windows 11, 64-bit.
- The Microsoft Edge WebView2 runtime. It is already installed on almost every Windows 10 and 11 computer. If MeetCue reports [MC-SYS-001](/troubleshooting/error-codes#mc-sys-001), install the Evergreen runtime from Microsoft.
- An API key for a speech recognition provider and one for a language model provider. See [Providers and keys](/guide/providers).

::: info Linux
MeetCue builds on Linux, but its native audio capture backend is not implemented yet.
:::

## Install on macOS

Requires macOS 13 Ventura or later, on Apple Silicon or Intel. Download `meetcue-macos-arm64.dmg` (Apple Silicon) or `meetcue-macos-x64.dmg` (Intel) from [Releases](https://github.com/Amazingkenneth/meetcue/releases), open it and drag **MeetCue.app** to **Applications**. CI also provides app ZIPs on pull requests for testing before a release.

The current Mac packages are ad-hoc signed and are not notarized. If macOS blocks an app you downloaded from this repository, open **System Settings › Privacy & Security** and choose **Open Anyway** for MeetCue. Do not disable Gatekeeper globally.

On first use, allow **Microphone** and **Screen & System Audio Recording** (called **Screen Recording** on older macOS) in **System Settings › Privacy & Security**. Quit and reopen MeetCue after granting screen/audio access. ScreenCaptureKit supplies system sound; MeetCue does not record screen images, and no virtual audio driver is needed. Microphone selection follows the system default unless you choose a specific device in Settings › Audio.

To try without keys, run:

```sh
/Applications/MeetCue.app/Contents/MacOS/MeetCue --demo
```

For the audio and shortcut checks, run the same executable with `--self-test`, with headphones connected, and speak during the microphone test. API keys use macOS Keychain. Updating replaces the app in Applications and keeps your settings and keys. To remove them, use **Settings › General › Erase all data**, then delete the app.

## Install with Scoop

1. If you don't have Scoop yet, open PowerShell and install it:

   ```powershell
   Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
   irm get.scoop.sh | iex
   ```

2. Add the MeetCue bucket:

   ```powershell
   scoop bucket add meetcue https://github.com/Amazingkenneth/meetcue
   ```

3. Install MeetCue:

   ```powershell
   scoop install meetcue
   ```

4. Start **MeetCue** from the Start menu. The first-run setup opens; continue with [First-time setup](/setup).

## Update

```powershell
scoop update meetcue
```

Your settings, latency history and practice records are kept in Scoop's `persist` folder, and API keys stay in Windows Credential Manager, so updates keep everything.

## Uninstall

```powershell
scoop uninstall meetcue
```

To also remove your settings, add `--purge`. API keys remain in Windows Credential Manager under entries named `MeetCue`; remove them there if you want, or run **Settings › General › Erase all data** before uninstalling to remove keys and settings together.

## If something goes wrong

See [Install and upgrade troubleshooting](/troubleshooting/install).
