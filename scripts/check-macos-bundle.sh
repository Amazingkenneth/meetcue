#!/usr/bin/env bash
# Verify the bundle we ship, rather than just an unbundled executable.
set -euo pipefail
app=${1:?Usage: check-macos-bundle.sh path/to/MeetCue.app}
plist="$app/Contents/Info.plist"
for key in NSMicrophoneUsageDescription NSAudioCaptureUsageDescription NSScreenCaptureUsageDescription; do
  test -n "$(/usr/libexec/PlistBuddy -c "Print :$key" "$plist")"
done
test "$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$plist")" = '13.0'
version=$("$app/Contents/MacOS/MeetCue" --version)
[[ "$version" =~ ^MeetCue\ [0-9]+\.[0-9]+\.[0-9]+$ ]]
codesign --verify --deep --strict "$app"
entitlements=$(mktemp)
trap 'rm -f "$entitlements"' EXIT
codesign --display --entitlements - "$app" > "$entitlements" 2>/dev/null
test "$(/usr/libexec/PlistBuddy -c 'Print :com.apple.security.device.audio-input' "$entitlements")" = 'true'
echo "Verified $version bundle and privacy metadata"
