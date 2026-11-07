#!/usr/bin/env bash
# Writes the browser extension of a built CursorPie as two zips, for the stores:
#   target/extension/cursorpie-bridge-chromium-<version>.zip   (Chrome Web Store, Edge Add-ons)
#   target/extension/cursorpie-bridge-firefox-<version>.zip    (sign it with: web-ext sign --source-dir target/extension/firefox)
# Usage: scripts/package-extension.sh [path to the cursorpie binary]   (default: target/release/cursorpie)
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="${1:-target/release/cursorpie}"
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
OUT="target/extension"

[ -x "$BIN" ] || { echo "build first: cargo build --release ($BIN not found)" >&2; exit 1; }
rm -rf "$OUT"
"$BIN" --export-extension "$OUT"
for family in chromium firefox; do
    (cd "$OUT/$family" && zip -qr "../cursorpie-bridge-$family-$VERSION.zip" .)
done
ls -l "$OUT"/*.zip
