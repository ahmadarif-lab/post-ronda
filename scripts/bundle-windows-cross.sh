#!/bin/sh
# Builds the Windows x64 package from Linux (used inside the cross-check container).
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked --target x86_64-pc-windows-gnu
PKG=dist/Post-Ronda-windows-x64
rm -rf "$PKG" dist/Post-Ronda-windows-x64.zip
mkdir -p "$PKG"
cp target/x86_64-pc-windows-gnu/release/post-ronda.exe "$PKG/PostRonda.exe"
cp README.md LICENSE assets/Inter-LICENSE.txt assets/Tabler-LICENSE.txt "$PKG/"
[ -f assets/Bruno-LICENSE.txt ] && cp assets/Bruno-LICENSE.txt "$PKG/"
(cd dist && zip -qr Post-Ronda-windows-x64.zip Post-Ronda-windows-x64)
printf 'Built %s\n' dist/Post-Ronda-windows-x64.zip
