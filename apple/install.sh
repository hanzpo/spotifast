#!/bin/bash
# Build SwiftSpot, the SwiftUI app with its Rust engine inside, and install
# it into /Applications.
#
#   apple/install.sh
#
# The app is one bundle: the SwiftUI front end, and in
# Contents/Helpers/SwiftSpot Engine.app the engine, which keeps the Spotify
# session and plays the audio. The front end starts the engine out of
# sight and the engine quits with it. The app is com.hanzpo.swiftspot and
# the engine com.hanzpo.swiftspot.engine; the engine keeps the saved
# sign-ins in the keychain under the first.
#
# The icon is SwiftSpot's own, Packaging/AppIcon.svg, rendered to
# Packaging/AppIcon-1024.png by Packaging/render-icon.swift; set ICON to
# another .icns file to use that instead. Set APP_DIR to
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

if [ -z "${ICON:-}" ]; then
    iconset="$(mktemp -d)/AppIcon.iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
        double=$((size * 2))
        sips -z $size $size "$here/Packaging/AppIcon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
        sips -z $double $double "$here/Packaging/AppIcon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    ICON="${iconset%.iconset}.icns"
    iconutil -c icns "$iconset" -o "$ICON"
fi

# The engine: the Rust app, bundled as packaging/macos/bundle.sh bundles
# it, kept out of the Dock.
(cd "$root" && cargo build --release --locked)
engine="$here/.build/SwiftSpot Engine.app"
"$root/packaging/macos/bundle.sh" "$root/target/release/spotifast" "$engine" "$version" >/dev/null
/usr/libexec/PlistBuddy -c "Add :LSUIElement bool true" "$engine/Contents/Info.plist"
cp "$ICON" "$engine/Contents/Resources/spotifast.icns"
codesign --force --sign "$identity" "$engine"

# The front end around it.
(cd "$here" && swift build -c release)
bundle="$here/.build/SwiftSpot.app"
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" "$bundle/Contents/Helpers"
cp "$(cd "$here" && swift build -c release --show-bin-path)/SwiftSpot" "$bundle/Contents/MacOS/SwiftSpot"
sed "s/__VERSION__/$version/g" "$here/Packaging/Info.plist" > "$bundle/Contents/Info.plist"
cp "$ICON" "$bundle/Contents/Resources/AppIcon.icns"
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
