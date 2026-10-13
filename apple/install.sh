#!/bin/bash
# Build SwiftSpot, the SwiftUI app with its Rust engine inside, and install
# it into /Applications.
#
#   apple/install.sh
#
# The app is one bundle: the SwiftUI front end, and in
# Contents/Helpers/SwiftSpot Engine.app the engine, which keeps the Spotify
# session and plays the audio. The front end starts the engine out of
# sight and the engine quits with it. The engine keeps the identifier the
# keychain already trusts for saved sign-ins, rocks.spotifast.Spotifast,
# and the same signing identity, so they open without a prompt.
#
# The icon comes from the Spotify app installed on this Mac when there is
# one, read at install time so it never enters the repository; set ICON to
# another .icns file, or to an empty string for none. Set APP_DIR to
# install somewhere other than /Applications. The bundle is signed with
# this Mac's Apple Development certificate when it has one, or
# CODESIGN_IDENTITY when set ("-" signs ad hoc). A running SwiftSpot is quit
# first and the new one opened afterwards. BUILD_ONLY=1 builds and checks
# the bundle without installing it.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"

app_dir="${APP_DIR:-/Applications}"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n1)"
if [ -z "${CODESIGN_IDENTITY:-}" ]; then
    CODESIGN_IDENTITY="$(security find-identity -v -p codesigning \
        | sed -n 's/^ *[0-9]*) \([0-9A-F]\{40\}\) "Apple Development: .*"$/\1/p' \
        | head -n1)"
fi
identity="${CODESIGN_IDENTITY:--}"

spotify_icon=/Applications/Spotify.app/Contents/Resources/AppIcon.icns
if [ -z "${ICON+set}" ] && [ -f "$spotify_icon" ]; then
    ICON=$spotify_icon
fi

# The engine: the Rust app, bundled as packaging/macos/bundle.sh bundles
# it, kept out of the Dock.
(cd "$root" && cargo build --release --locked)
engine="$here/.build/SwiftSpot Engine.app"
"$root/packaging/macos/bundle.sh" "$root/target/release/spotifast" "$engine" "$version" >/dev/null
/usr/libexec/PlistBuddy -c "Add :LSUIElement bool true" "$engine/Contents/Info.plist"
if [ -n "${ICON:-}" ]; then
    cp "$ICON" "$engine/Contents/Resources/spotifast.icns"
fi
codesign --force --sign "$identity" "$engine"

# The front end around it.
(cd "$here" && swift build -c release)
bundle="$here/.build/SwiftSpot.app"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$bundle/Contents/Helpers"
cp "$(cd "$here" && swift build -c release --show-bin-path)/SwiftSpot" "$bundle/Contents/MacOS/SwiftSpot"
sed "s/__VERSION__/$version/g" "$here/Packaging/Info.plist" > "$bundle/Contents/Info.plist"
if [ -n "${ICON:-}" ]; then
    cp "$ICON" "$bundle/Contents/Resources/AppIcon.icns"
fi
mv "$engine" "$bundle/Contents/Helpers/"
codesign --force --sign "$identity" "$bundle"
codesign --verify --deep --strict "$bundle"

# BUILD_ONLY stops here, with the bundle built and checked.
if [ -n "${BUILD_ONLY:-}" ]; then
    echo "$bundle"
    exit 0
fi

# Quit what runs now: this app, and the engine or the app under its older
# names, Spotlite and Spotifast.
for name in SwiftSpot Spotlite Spotifast; do
    if pgrep -xq "$name"; then
        osascript -e "quit app \"$name\"" 2>/dev/null || true
        for _ in $(seq 20); do
            pgrep -xq "$name" || break
            sleep 0.5
        done
        pkill -x "$name" 2>/dev/null || true
    fi
done

rm -rf "$app_dir/SwiftSpot.app" "$app_dir/Spotlite.app" "$app_dir/Spotifast.app"
cp -R "$bundle" "$app_dir/"
codesign --verify --deep --strict "$app_dir/SwiftSpot.app"
open "$app_dir/SwiftSpot.app"
echo "Installed SwiftSpot $version in $app_dir"
