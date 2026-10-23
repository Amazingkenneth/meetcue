# Install and upgrade

This page helps when Scoop can't install or update MeetCue, when Windows blocks it, or when settings seem lost after an update.

## Scoop can't download MeetCue {#download}

**Possible causes**

1. No internet access to `github.com`, or a proxy is required.
2. The bucket was added with a wrong address.

**Check**

1. Run `scoop bucket list` and check the address of the `meetcue` bucket.
2. Open `https://github.com/Amazingkenneth/meetcue/releases` in a browser.

**Fix**

1. Re-add the bucket:

   ```powershell
   scoop bucket rm meetcue
   scoop bucket add meetcue https://github.com/Amazingkenneth/meetcue
   ```

2. Behind a proxy, configure Scoop with `scoop config proxy proxy.example.com:8080`.

**Still not solved?** [Open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml) with the full Scoop output.

## Scoop reports a hash mismatch {#hash}

**Possible causes**

1. A download was interrupted or altered by a proxy.
2. Your local bucket is out of date.

**Check**

1. Run `scoop update` to refresh all buckets.

**Fix**

1. Clear the cached download and try again:

   ```powershell
   scoop cache rm meetcue
   scoop install meetcue
   ```

**Still not solved?** [Open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml) with the full Scoop output.

## Windows or antivirus blocks MeetCue {#blocked}

**Possible causes**

1. MeetCue isn't code-signed in this release, so SmartScreen may warn about an unknown publisher.
2. Antivirus software flags a new, rarely downloaded program.
3. Company policy only allows approved programs.

**Check**

1. Note the exact message and which program shows it.

**Fix**

1. For SmartScreen, click **More info › Run anyway** if you trust the download. The release page lists the SHA-256 hash of the zip so you can compare it.
2. Report a false positive to your antivirus vendor, or ask IT to allow `MeetCue.exe`.
3. If company policy blocks unsigned programs, MeetCue can't run there until signed builds are available.

**Still not solved?** [Open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## MeetCue reports that WebView2 is missing {#webview2}

**Possible causes**

1. The Microsoft Edge WebView2 runtime was removed or never installed ([MC-SYS-001](/troubleshooting/error-codes#mc-sys-001)).

**Check**

1. Look for **Microsoft Edge WebView2 Runtime** in **Settings › Apps › Installed apps**.

**Fix**

1. Download and install the Evergreen WebView2 runtime from Microsoft, then start MeetCue again.

**Still not solved?** [Open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).

## Settings are gone after an update {#settings-lost}

**Possible causes**

1. MeetCue couldn't write to its folder and saved settings in `%APPDATA%\MeetCue` instead ([MC-SYS-002](/troubleshooting/error-codes#mc-sys-002)).
2. MeetCue was uninstalled with `--purge`.

**Check**

1. Open **Settings › Diagnostics › About** and look at **Settings folder**. With Scoop it should be under `scoop\persist\meetcue\config`.

**Fix**

1. If the settings are in `%APPDATA%\MeetCue`, copy `settings.json` and `latency-history.json` into the persist folder, then restart MeetCue.
2. API keys stay in Windows Credential Manager and survive updates and uninstalls.

**Still not solved?** [Export diagnostics](/troubleshooting/diagnostics) and [open an issue](https://github.com/Amazingkenneth/meetcue/issues/new?template=bug.yml).
