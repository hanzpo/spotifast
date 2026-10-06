#!/bin/bash
# Build Spotifast from this checkout and install it into /Applications.
#
#   packaging/macos/install.sh
#
# Set ICON to an .icns file to use it as the app icon instead of the
# bundled one. Set APP_DIR to install somewhere other than /Applications.
# A running copy is quit first and the new one is opened afterwards.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

app_dir="${APP_DIR:-/Applications}"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)"
bundle="target/Spotifast.app"

cargo build --release --locked
packaging/macos/bundle.sh target/release/spotifast "$bundle" "$version"

if [ -n "${ICON:-}" ]; then
    cp "$ICON" "$bundle/Contents/Resources/spotifast.icns"
    # Replacing a resource breaks the signature bundle.sh made.
    codesign --force --deep --sign "${CODESIGN_IDENTITY:--}" "$bundle"
fi

if pgrep -xq Spotifast; then
    osascript -e 'quit app "Spotifast"'
    for _ in $(seq 20); do
        pgrep -xq Spotifast || break
        sleep 0.5
    done
fi

rm -rf "$app_dir/Spotifast.app"
cp -R "$bundle" "$app_dir/"
codesign --verify "$app_dir/Spotifast.app"
open "$app_dir/Spotifast.app"
echo "Installed Spotifast $version in $app_dir"
