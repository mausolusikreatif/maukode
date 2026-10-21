#!/usr/bin/env bash
# MauKode installer (Linux x86_64). Downloads a release tarball from GitHub, verifies its
# checksum, and installs the daemon and web UI into ~/.local (no root needed).
#
#   curl -fsSL https://raw.githubusercontent.com/mausolusikreatif/maukode/master/install.sh | bash
#   ... | bash -s -- --version v0.5.0     # a specific release
#   ... | bash -s -- --uninstall          # remove it
#
# PREFIX=/some/dir changes the install location (default: ~/.local).
set -euo pipefail

repo="mausolusikreatif/maukode"
version=""
uninstall=0

while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:?--version needs a tag, e.g. v0.5.0}"; shift 2 ;;
    --uninstall) uninstall=1; shift ;;
    -h|--help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

if [ "$uninstall" = 1 ]; then
  prefix="${PREFIX:-$HOME/.local}"
  "$prefix/bin/maukode" server stop >/dev/null 2>&1 || true
  rm -f "$prefix/bin/maukode"; rm -rf "$prefix/share/maukode/web"
  echo "removed"; exit 0
fi

[ "$(uname -s)" = "Linux" ] && [ "$(uname -m)" = "x86_64" ] || {
  echo "only Linux x86_64 release builds exist; build from source instead (see README)" >&2
  exit 1
}
for tool in curl tar sha256sum; do
  command -v "$tool" >/dev/null || { echo "missing required tool: $tool" >&2; exit 1; }
done

if [ -z "$version" ]; then
  # /releases/latest redirects to /releases/tag/<tag>; no API token or rate limit involved.
  version="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")"
  version="${version##*/}"
fi
case "$version" in v[0-9]*) ;; *) echo "could not resolve a release version (got '$version')" >&2; exit 1 ;; esac

name="maukode-linux-x86_64-$version"
base="https://github.com/$repo/releases/download/$version"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT

echo "downloading MauKode $version"
curl -fsSL -o "$tmp/$name.tar.gz" "$base/$name.tar.gz"
curl -fsSL -o "$tmp/$name.tar.gz.sha256" "$base/$name.tar.gz.sha256"
(cd "$tmp" && sha256sum -c "$name.tar.gz.sha256")

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
"$tmp/$name/install.sh"
