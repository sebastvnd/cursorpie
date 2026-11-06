#!/usr/bin/env bash
# Builds target/bundle/CursorPie.app (menu-bar only app, no Dock icon).
# Usage:  scripts/bundle.sh                 (ad-hoc signature, this machine's architecture)
#         SIGN_ID="Developer ID Application: Name (TEAMID)" scripts/bundle.sh
#         UNIVERSAL=1 scripts/bundle.sh     (one app for Apple silicon and Intel)
# To get a .dmg to hand out, run scripts/make-dmg.sh (it calls this script first).
set -euo pipefail
cd "$(dirname "$0")/.."

APP="CursorPie"
BUNDLE_ID="io.github.cursorpie"   # keep in sync with src/about.rs
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')

if [ "${UNIVERSAL:-0}" = "1" ]; then
    # one binary that runs natively on Apple silicon and on Intel
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    BIN="target/CursorPie-universal"
    lipo -create -output "$BIN" \
        target/aarch64-apple-darwin/release/cursorpie \
        target/x86_64-apple-darwin/release/cursorpie
else
    cargo build --release
    BIN="target/release/cursorpie"
fi
OUT="target/bundle/$APP.app"
rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp "$BIN" "$OUT/Contents/MacOS/$APP"
cp assets/app-icon.icns "$OUT/Contents/Resources/CursorPie.icns"

cat > "$OUT/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>$APP</string>
  <key>CFBundleDisplayName</key><string>$APP</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>$APP</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>CursorPie.icns</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>NSHumanReadableCopyright</key><string>Copyright (c) 2026 Sebastvnd</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

# Finder metadata and quarantine flags inside the bundle break the signature ("app is damaged").
xattr -cr "$OUT" 2>/dev/null || true

SIGN_ARGS=(--force --deep --sign "${SIGN_ID:--}")
if [ -n "${SIGN_ID:-}" ]; then
    # a Developer ID signature needs the hardened runtime and a timestamp to be notarized
    SIGN_ARGS+=(--options runtime --timestamp)
else
    # Ad-hoc signature: pin the identity to the bundle id instead of the binary hash. Without
    # this, macOS treats every rebuild as a new app and forgets the Accessibility / Input
    # Monitoring permission, so the app starts at login but its event tap stays dead.
    SIGN_ARGS+=(-r="designated => identifier \"$BUNDLE_ID\"")
fi
codesign "${SIGN_ARGS[@]}" "$OUT"
codesign --verify --deep --strict "$OUT"
echo "Built $OUT ($(lipo -archs "$OUT/Contents/MacOS/$APP"))"
echo "Note: with the ad-hoc signature macOS may ask you to re-grant Accessibility after every rebuild."
