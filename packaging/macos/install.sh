#!/bin/bash
# Build Spotlite from this checkout and install it into /Applications.
#
#   packaging/macos/install.sh
#
# The icon comes from the Spotify app installed on this Mac when there is
# one, read at install time so it never enters the repository. Set ICON to
# another .icns file to use that instead, or to an empty string to keep the
# bundled icon. Set APP_DIR to install somewhere other than /Applications.
# A running copy is quit first and the new one is opened afterwards.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

app_dir="${APP_DIR:-/Applications}"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)"
bundle="target/Spotlite.app"

cargo build --release --locked
packaging/macos/bundle.sh target/release/spotifast "$bundle" "$version"

spotify_icon=/Applications/Spotify.app/Contents/Resources/AppIcon.icns
if [ -z "${ICON+set}" ] && [ -f "$spotify_icon" ]; then
    ICON=$spotify_icon
fi
if [ -n "${ICON:-}" ]; then
    cp "$ICON" "$bundle/Contents/Resources/spotifast.icns"
    # Replacing a resource breaks the signature bundle.sh made.
    codesign --force --deep --sign "${CODESIGN_IDENTITY:--}" "$bundle"
fi

# Spotifast is the name the app had before; replace that copy too.
for name in Spotlite Spotifast; do
    if pgrep -xq "$name"; then
        osascript -e "quit app \"$name\""
        for _ in $(seq 20); do
            pgrep -xq "$name" || break
            sleep 0.5
        done
    fi
done

rm -rf "$app_dir/Spotlite.app" "$app_dir/Spotifast.app"
cp -R "$bundle" "$app_dir/"
codesign --verify "$app_dir/Spotlite.app"
open "$app_dir/Spotlite.app"
echo "Installed Spotlite $version in $app_dir"
