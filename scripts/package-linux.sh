#!/usr/bin/env bash
# Builds target/bundle/cursorpie-<version>-linux-<arch>.tar.gz with the program, its desktop entry,
# its icon and an install.sh that copies them under a prefix (default /usr/local, or ~/.local with
# --user).
# Usage:  scripts/package-linux.sh
# Needs the development files of libx11, libxi, libxtst and libxrandr (see CONTRIBUTING.md).
# The icons (16 to 512 px, same artwork as the Windows and macOS icons) are in assets/icons; no
# image-conversion tool is needed.
# Build on an older distribution if the program must start on older ones (glibc is not backwards
# compatible).
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
ARCH=$(uname -m)
NAME="cursorpie-$VERSION-linux-$ARCH"
OUT="target/bundle/$NAME"

cargo build --release
rm -rf "$OUT" "$OUT.tar.gz"
mkdir -p "$OUT"
cp target/release/cursorpie "$OUT/cursorpie"
cp packaging/linux/cursorpie.desktop "$OUT/cursorpie.desktop"
cp LICENSE README.md "$OUT/"

# Keep the white mark on the sage background square; do not use the horizontal wordmark here.
mkdir -p "$OUT/icons"
cp assets/icons/cursorpie-*.png "$OUT/icons/"

cat > "$OUT/install.sh" <<'INSTALL'
#!/usr/bin/env sh
# Installs CursorPie. Usage: ./install.sh [--user | --prefix DIR]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
prefix=/usr/local
case "${1:-}" in
    --user) prefix="${XDG_DATA_HOME:-$HOME/.local/share}/.."; prefix=$(cd "$prefix" 2>/dev/null && pwd || echo "$HOME/.local") ;;
    --prefix) prefix=${2:?--prefix needs a directory} ;;
    "") ;;
    *) echo "usage: $0 [--user | --prefix DIR]" >&2; exit 2 ;;
esac
install -Dm755 "$here/cursorpie" "$prefix/bin/cursorpie"
install -Dm644 "$here/cursorpie.desktop" "$prefix/share/applications/cursorpie.desktop"
for f in "$here"/icons/cursorpie-*.png; do
    [ -f "$f" ] || continue
    size=$(basename "$f" .png); size=${size#cursorpie-}
    install -Dm644 "$f" "$prefix/share/icons/hicolor/${size}x${size}/apps/cursorpie.png"
done
# make the desktop pick up the new icon and entry without a logout (each tool is optional)
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t -f "$prefix/share/icons/hicolor" 2>/dev/null || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
fi
echo "Installed under $prefix. Start CursorPie from your applications menu or run: cursorpie"
INSTALL
chmod +x "$OUT/install.sh"

tar -C target/bundle -czf "$OUT.tar.gz" "$NAME"
echo "Built $OUT.tar.gz"
