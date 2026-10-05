#!/usr/bin/env bash
# Smoke test for the real desktop shell: launch the debug app under a virtual display and check
# that the UI, through Tauri IPC (`daemon_info`), makes the shell start an authenticated daemon
# and that the app then shows the connected UI.
#
# Needs: the Tauri Linux prerequisites, xvfb, imagemagick, and a built `maukode` CLI
# (`cargo build -p maukode-cli`, or set MAUKODE_BIN).
set -euo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(cd ../.. && pwd)"
WORK="$(mktemp -d)"
export MAUKODE_HOME="$WORK/home"
export MAUKODE_BIN="${MAUKODE_BIN:-$REPO_ROOT/target/debug/maukode}"
export DISPLAY=":${SMOKE_DISPLAY:-99}"
# Software rendering: virtual displays have no GPU.
export WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1
SHOT="${SMOKE_SCREENSHOT:-$WORK/shell.png}"

pids=()
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  "$MAUKODE_BIN" server stop >/dev/null 2>&1 || true
  if [ -z "${SMOKE_KEEP:-}" ]; then rm -rf "$WORK"; else echo "kept $WORK"; fi
}
trap cleanup EXIT

fail() { echo "SMOKE FAIL: $*" >&2; for f in vite app; do echo "--- $f.log"; tail -30 "$WORK/$f.log" 2>/dev/null || true; done; exit 1; }
wait_for() { # description, command...
  local what="$1"; shift
  for _ in $(seq 1 120); do "$@" >/dev/null 2>&1 && return 0; sleep 1; done
  fail "timed out waiting for $what"
}

[ -x "$MAUKODE_BIN" ] || fail "maukode binary not found at $MAUKODE_BIN"

Xvfb "$DISPLAY" -screen 0 1280x800x24 >/dev/null 2>&1 &
pids+=($!)
wait_for "virtual display" xdpyinfo -display "$DISPLAY"

# Debug builds load the dev server URL from tauri.conf.json.
# Run vite directly (not via npx) so the recorded pid is the process we need to stop.
./node_modules/.bin/vite --host 127.0.0.1 --port 1420 --strictPort >"$WORK/vite.log" 2>&1 &
pids+=($!)
wait_for "vite dev server" curl -sf http://127.0.0.1:1420/

(cd src-tauri && cargo build --locked)
src-tauri/target/debug/maukode-desktop >"$WORK/app.log" 2>&1 &
pids+=($!)

DAEMON_JSON="$MAUKODE_HOME/data/maukode/daemon.json"
wait_for "the shell to start the daemon ($DAEMON_JSON)" test -s "$DAEMON_JSON"

URL="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["url"])' "$DAEMON_JSON")"
TOKEN="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["token"] or "")' "$DAEMON_JSON")"
[ -n "$TOKEN" ] || fail "daemon was started without a token"

[ "$(stat -c %a "$DAEMON_JSON")" = "600" ] || fail "daemon.json is not private (0600)"
[ "$(curl -s -o /dev/null -w '%{http_code}' "$URL/api/v1/sessions")" = "401" ] || fail "daemon accepted an unauthenticated request"
[ "$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $TOKEN" "$URL/api/v1/sessions")" = "200" ] || fail "token was rejected"
if ps -eo args | grep '[m]aukode server' | grep -q -- "$TOKEN"; then fail "token is visible on the daemon's command line"; fi

# The UI must have used that token: the connected layout shows the sidebar.
sleep 4
import -display "$DISPLAY" -window root "$SHOT"
echo "screenshot: $SHOT"
grep -qi "panic\|error while running" "$WORK/app.log" && fail "the app logged a failure"

# The sidebar's connection indicator (top right of the sidebar) is green once connected.
PIXEL="$(convert "$SHOT" -crop 1x1+262+22 +repage -depth 8 txt:- | tail -1 | grep -o '([0-9, ]*)' | tr -d '() ')"
IFS=, read -r R G B _ <<<"$PIXEL"
if ! { [ "${G:-0}" -ge $((${R:-0} + 30)) ] && [ "${G:-0}" -ge $((${B:-0} + 10)) ]; }; then
  fail "UI does not look connected (indicator pixel rgb=$R,$G,$B; expected green)"
fi
echo "SMOKE OK: shell started an authenticated daemon at $URL"
