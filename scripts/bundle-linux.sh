#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
ARCH=$(uname -m)
PKG="dist/post-ronda-linux-$ARCH"
mkdir -p "$PKG/bin" "$PKG/share/applications" "$PKG/share/icons/hicolor/256x256/apps" "$PKG/share/licenses/post-ronda"
cp target/release/post-ronda "$PKG/bin/"
strip "$PKG/bin/post-ronda"
cp assets/icon-256.png "$PKG/share/icons/hicolor/256x256/apps/post-ronda.png"
cp LICENSE assets/Inter-LICENSE.txt assets/Tabler-LICENSE.txt "$PKG/share/licenses/post-ronda/"
[ -f assets/Bruno-LICENSE.txt ] && cp assets/Bruno-LICENSE.txt "$PKG/share/licenses/post-ronda/"
cat > "$PKG/share/applications/post-ronda.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Post Ronda
Comment=Native HTTP client with local collections
Exec=post-ronda
Icon=post-ronda
Terminal=false
Categories=Development;Network;
DESKTOP
cp README.md "$PKG/"
tar -czf "dist/post-ronda-linux-$ARCH.tar.gz" -C dist "post-ronda-linux-$ARCH"
printf 'Built %s\n' "dist/post-ronda-linux-$ARCH.tar.gz"
