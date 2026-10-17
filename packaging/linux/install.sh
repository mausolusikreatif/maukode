#!/usr/bin/env bash
# Installs MauKode into ~/.local (no root needed). Uninstall: ./install.sh --uninstall
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
files=("$prefix/bin/maukode")
web="$prefix/share/maukode/web"

if [ "${1:-}" = "--uninstall" ]; then
  "$prefix/bin/maukode" server stop >/dev/null 2>&1 || true
  rm -f "${files[@]}"; rm -rf "$web"; echo "removed"; exit 0
fi

install -Dm755 "$here/maukode" "$prefix/bin/maukode"
rm -rf "$web"; mkdir -p "$web"; cp -r "$here/web/." "$web/"

echo "installed to $prefix (make sure $prefix/bin is in PATH)"
echo "serve the web UI: maukode server --bind 127.0.0.1:4096 --token <token> --web-dir $web"
