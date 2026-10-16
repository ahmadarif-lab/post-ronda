#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
APP="dist/Post Ronda.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/post-ronda "$APP/Contents/MacOS/PostRonda"
strip -x -S "$APP/Contents/MacOS/PostRonda"
if [ ! -f assets/PostRonda.icns ]; then
    ICONSET=$(mktemp -d)/PostRonda.iconset
    mkdir -p "$ICONSET"
    for SIZE in 16 32 128 256 512; do
        sips -s format png -z "$SIZE" "$SIZE" assets/icon.webp --out "$ICONSET/icon_${SIZE}x${SIZE}.png" >/dev/null
        DOUBLE=$((SIZE * 2))
        sips -s format png -z "$DOUBLE" "$DOUBLE" assets/icon.webp --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICONSET" -o assets/PostRonda.icns
fi
cp assets/PostRonda.icns "$APP/Contents/Resources/"
cp assets/Inter-LICENSE.txt assets/Tabler-LICENSE.txt "$APP/Contents/Resources/"
rm -f "$APP/Contents/Resources/Bruno-LICENSE.txt"
[ -f assets/Bruno-LICENSE.txt ] && cp assets/Bruno-LICENSE.txt "$APP/Contents/Resources/"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Post Ronda</string>
<key>CFBundleDisplayName</key><string>Post Ronda</string>
<key>CFBundleIdentifier</key><string>id.postronda.desktop</string>
<key>CFBundleExecutable</key><string>PostRonda</string>
<key>CFBundleIconFile</key><string>PostRonda</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
<key>NSRequiresAquaSystemAppearance</key><false/>
<key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
</dict></plist>
PLIST
codesign --force --deep --sign - "$APP"
printf 'Built %s\n' "$APP"
du -sh "$APP"
