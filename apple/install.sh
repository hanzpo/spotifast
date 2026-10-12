#!/bin/bash
# Build Spotlite Swift and install it into /Applications.
#
#   apple/install.sh
#
# The app is a front end: it drives the installed Spotlite, which keeps the
# Spotify session and plays the audio, and starts it in the background when
# it isn't running. Install Spotlite first (packaging/macos/install.sh).
#
# The icon comes from the Spotify app installed on this Mac when there is
# one, read at install time so it never enters the repository; set ICON to
# another .icns file, or to an empty string for none. Set APP_DIR to
# install somewhere other than /Applications. The bundle is signed with
# this Mac's Apple Development certificate when it has one, or
# CODESIGN_IDENTITY when set ("-" signs ad hoc).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
cd "$here"

app_dir="${APP_DIR:-/Applications}"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n1)"
bundle="$here/.build/Spotlite Swift.app"
if [ -z "${CODESIGN_IDENTITY:-}" ]; then
    CODESIGN_IDENTITY="$(security find-identity -v -p codesigning \
        | sed -n 's/^ *[0-9]*) \([0-9A-F]\{40\}\) "Apple Development: .*"$/\1/p' \
        | head -n1)"
fi
identity="${CODESIGN_IDENTITY:--}"

swift build -c release
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$(swift build -c release --show-bin-path)/SpotliteSwift" "$bundle/Contents/MacOS/"
sed "s/__VERSION__/$version/g" Packaging/Info.plist > "$bundle/Contents/Info.plist"

spotify_icon=/Applications/Spotify.app/Contents/Resources/AppIcon.icns
if [ -z "${ICON+set}" ] && [ -f "$spotify_icon" ]; then
    ICON=$spotify_icon
fi
if [ -n "${ICON:-}" ]; then
    cp "$ICON" "$bundle/Contents/Resources/AppIcon.icns"
fi
codesign --force --sign "$identity" "$bundle"

if pgrep -xq SpotliteSwift; then
    osascript -e 'quit app "Spotlite Swift"'
    for _ in $(seq 20); do
        pgrep -xq SpotliteSwift || break
        sleep 0.5
    done
fi
rm -rf "$app_dir/Spotlite Swift.app"
cp -R "$bundle" "$app_dir/"
codesign --verify "$app_dir/Spotlite Swift.app"
open "$app_dir/Spotlite Swift.app"
echo "Installed Spotlite Swift $version in $app_dir"
