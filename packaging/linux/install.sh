#!/usr/bin/env bash
# Installs MauKode into ~/.local (no root needed). Uninstall: ./install.sh --uninstall
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
files=("$prefix/bin/maukode" "$prefix/bin/maukode-desktop"
  "$prefix/share/applications/maukode-desktop.desktop" "$prefix/share/icons/hicolor/128x128/apps/maukode.png")

if [ "${1:-}" = "--uninstall" ]; then
  "$prefix/bin/maukode" server stop >/dev/null 2>&1 || true
  rm -f "${files[@]}"; echo "removed"; exit 0
fi

install -Dm755 "$here/maukode" "$prefix/bin/maukode"
install -Dm755 "$here/maukode-desktop" "$prefix/bin/maukode-desktop"
install -Dm644 "$here/maukode.png" "$prefix/share/icons/hicolor/128x128/apps/maukode.png"
sed "s|^Exec=.*|Exec=$prefix/bin/maukode-desktop|" "$here/maukode-desktop.desktop" \
  > "$here/.desktop.tmp"
install -Dm644 "$here/.desktop.tmp" "$prefix/share/applications/maukode-desktop.desktop"
rm -f "$here/.desktop.tmp"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true

echo "installed to $prefix (make sure $prefix/bin is in PATH)"
ldd "$prefix/bin/maukode-desktop" | grep "not found" && echo "^ install these libraries (see README.txt)" || true
