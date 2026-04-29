#!/usr/bin/env bash
# Browser dev: free the WebSocket port, start tyutool-cli serve, then Vite.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${TYUTOOL_SERVE_PORT:-9527}"

free_tcp_port() {
  local p="$1"
  # Linux procps fuser: fuser -k PORT/tcp. BSD/macOS fuser has different syntax and
  # may print usage to stdout; use lsof on Darwin instead.
  if [[ "$(uname -s)" == "Linux" ]] && command -v fuser >/dev/null 2>&1; then
    fuser -k "${p}/tcp" >/dev/null 2>&1 || true
  fi
  if command -v lsof >/dev/null 2>&1; then
    local pids
    pids="$(lsof -tiTCP:"${p}" -sTCP:LISTEN 2>/dev/null || true)"
    if [[ -n "${pids}" ]]; then
      # shellcheck disable=SC2086
      kill -9 ${pids} 2>/dev/null || true
    fi
  fi
}

free_tcp_port "${PORT}"

cargo build -q -p tyutool-cli
cargo run -p tyutool-cli -- serve --port "${PORT}" &
export DEV_WEB_LOOSE_PORT=1
exec vite
