#!/bin/sh
# Builds dist/Post-Ronda-macos-<arch>.pkg, a standard installer that puts the app in /Applications.
# Run scripts/bundle-macos.sh first.
set -eu
cd "$(dirname "$0")/.."
APP="dist/Post Ronda.app"
[ -d "$APP" ] || { echo "Run scripts/bundle-macos.sh first" >&2; exit 1; }
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
case "$(uname -m)" in
    arm64) NAME=arm64; HOST=arm64 ;;
    *) NAME=x86_64; HOST=x86_64 ;;
esac
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/root" "$WORK/scripts" "$WORK/resources"
ditto "$APP" "$WORK/root/Post Ronda.app"

# After installing, the app belongs to the person who installed it, so that the updater inside
# the app can replace it later without asking for a password.
cat > "$WORK/scripts/postinstall" <<'SCRIPT'
#!/bin/sh
OWNER=$(stat -f%Su /dev/console)
if [ -n "$OWNER" ] && [ "$OWNER" != root ]; then
    chown -R "$OWNER":admin "$2/Post Ronda.app" 2>/dev/null || true
fi
exit 0
SCRIPT
chmod +x "$WORK/scripts/postinstall"

# Replace an installed copy in place instead of relocating to wherever one is found.
pkgbuild --analyze --root "$WORK/root" "$WORK/components.plist" >/dev/null
/usr/libexec/PlistBuddy -c "Add :0:BundleIsRelocatable bool false" -c "Set :0:BundleIsVersionChecked false" "$WORK/components.plist"
pkgbuild --root "$WORK/root" --component-plist "$WORK/components.plist" --scripts "$WORK/scripts" \
    --identifier id.postronda.desktop --version "$VERSION" --install-location /Applications \
    "$WORK/PostRonda-app.pkg" >/dev/null 2>&1

cp LICENSE "$WORK/resources/LICENSE.txt"
cat > "$WORK/distribution.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>Post Ronda</title>
    <license file="LICENSE.txt"/>
    <options customize="never" require-scripts="false" hostArchitectures="$HOST"/>
    <domains enable_localSystem="true"/>
    <choices-outline><line choice="default"><line choice="id.postronda.desktop"/></line></choices-outline>
    <choice id="default"/>
    <choice id="id.postronda.desktop" visible="false"><pkg-ref id="id.postronda.desktop"/></choice>
    <pkg-ref id="id.postronda.desktop" version="$VERSION" onConclusion="none">PostRonda-app.pkg</pkg-ref>
</installer-gui-script>
XML
productbuild --distribution "$WORK/distribution.xml" --resources "$WORK/resources" \
    --package-path "$WORK" "dist/Post-Ronda-macos-$NAME.pkg" >/dev/null
printf 'Built %s\n' "dist/Post-Ronda-macos-$NAME.pkg"
