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
#
# The bundle is signed with this Mac's Apple Development certificate when it
# has one, or CODESIGN_IDENTITY when set ("-" signs ad hoc). The keychain
# trusts a signing identity, not a build, so Always Allow on its prompts
# lasts across reinstalls; an ad hoc signature is new with every build and
# asks again for each saved sign-in.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

app_dir="${APP_DIR:-/Applications}"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)"
bundle="target/Spotlite.app"
if [ -z "${CODESIGN_IDENTITY:-}" ]; then
    CODESIGN_IDENTITY="$(security find-identity -v -p codesigning \
        | sed -n 's/^ *[0-9]*) \([0-9A-F]\{40\}\) "Apple Development: .*"$/\1/p' \
        | head -n1)"
fi
identity="${CODESIGN_IDENTITY:--}"

cargo build --release --locked
packaging/macos/bundle.sh target/release/spotifast "$bundle" "$version"

spotify_icon=/Applications/Spotify.app/Contents/Resources/AppIcon.icns
if [ -z "${ICON+set}" ] && [ -f "$spotify_icon" ]; then
    ICON=$spotify_icon
fi
if [ -n "${ICON:-}" ]; then
    cp "$ICON" "$bundle/Contents/Resources/spotifast.icns"
fi
# bundle.sh signs ad hoc; sign again with the identity, after the icon.
codesign --force --deep --sign "$identity" "$bundle"

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
