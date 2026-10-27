# Install MeetCue

This page shows how to install, update and remove MeetCue on Windows. The whole process works without administrator rights.

## Requirements

- Windows 10 version 2004 or later, or Windows 11, 64-bit.
- The Microsoft Edge WebView2 runtime. It is already installed on almost every Windows 10 and 11 computer. If MeetCue reports [MC-SYS-001](/troubleshooting/error-codes#mc-sys-001), install the Evergreen runtime from Microsoft.
- An API key for a speech recognition provider and one for a language model provider. See [Providers and keys](/guide/providers).

::: info macOS and Linux
MeetCue builds on macOS and Linux, but audio capture is only available on Windows in this release.
:::

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
