#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
osxcross=${OSXCROSS_ROOT:-/run/media/lurenmax/Data/Tools/Apple/osxcross}
tools="$osxcross/target/bin"
darwin=25.5
app="$root/dist/ratablet.app"

export PATH="$tools:$PATH"
export MACOSX_DEPLOYMENT_TARGET=11.0
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="aarch64-apple-darwin${darwin}-clang"
export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="x86_64-apple-darwin${darwin}-clang"

command -v "$CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER" >/dev/null
command -v "$CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER" >/dev/null
command -v lipo >/dev/null

cd "$root"
cargo build --locked --release --target aarch64-apple-darwin
cargo build --locked --release --target x86_64-apple-darwin

rm -rf -- "$app"
install -d "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 644 packaging/macos/Info.plist "$app/Contents/Info.plist"
lipo -create \
    target/aarch64-apple-darwin/release/ratablet \
    target/x86_64-apple-darwin/release/ratablet \
    -output "$app/Contents/MacOS/ratablet"
chmod 755 "$app/Contents/MacOS/ratablet"

echo "Created $app"
lipo -info "$app/Contents/MacOS/ratablet"
