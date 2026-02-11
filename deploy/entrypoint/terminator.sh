#!/usr/bin/env bash
set -euo pipefail

TEMPLATE_PATH="${TEMPLATE_PATH:-/etc/tcpao-proxy/terminator.toml.tmpl}"
CONFIG_PATH="${CONFIG_PATH:-/etc/tcpao-proxy/config.toml}"

LISTEN_AO="${LISTEN_AO:-0.0.0.0:1790}"
FORWARD_PLAIN="${FORWARD_PLAIN:-127.0.0.1:11019}"
PEER_IP="${PEER_IP:-}"
PEER_PORT="${PEER_PORT:-1790}"
KEY_ID="${KEY_ID:-1}"
APP_CMD="${APP_CMD:-/gobmp}"

required_env() {
  local key="$1"
  if [[ -z "${!key:-}" ]]; then
    echo "[entrypoint] missing required env: ${key}" >&2
    exit 1
  fi
}

escape_awk_replacement() {
  printf '%s' "$1" | sed 's/[&\\]/\\&/g'
}

render_config() {
  local template="$1"
  local output="$2"

  local listen_ao forward_plain peer_ip peer_port key_id
  listen_ao="$(escape_awk_replacement "$LISTEN_AO")"
  forward_plain="$(escape_awk_replacement "$FORWARD_PLAIN")"
  peer_ip="$(escape_awk_replacement "$PEER_IP")"
  peer_port="$(escape_awk_replacement "$PEER_PORT")"
  key_id="$(escape_awk_replacement "$KEY_ID")"

  awk \
    -v LISTEN_AO="$listen_ao" \
    -v FORWARD_PLAIN="$forward_plain" \
    -v PEER_IP="$peer_ip" \
    -v PEER_PORT="$peer_port" \
    -v KEY_ID="$key_id" '
      {
        gsub(/\$\{LISTEN_AO\}/, LISTEN_AO);
        gsub(/\$\{FORWARD_PLAIN\}/, FORWARD_PLAIN);
        gsub(/\$\{PEER_IP\}/, PEER_IP);
        gsub(/\$\{PEER_PORT\}/, PEER_PORT);
        gsub(/\$\{KEY_ID\}/, KEY_ID);
        print;
      }
    ' "$template" > "$output"
}

shutdown() {
  local code="${1:-0}"
  if [[ -n "${APP_PID:-}" ]]; then
    kill "${APP_PID}" 2>/dev/null || true
  fi
  if [[ -n "${PROXY_PID:-}" ]]; then
    kill "${PROXY_PID}" 2>/dev/null || true
  fi
  wait "${APP_PID:-}" 2>/dev/null || true
  wait "${PROXY_PID:-}" 2>/dev/null || true
  exit "$code"
}

required_env TCPAO_KEY
required_env PEER_IP

mkdir -p "$(dirname "$CONFIG_PATH")"
if [[ ! -f "$TEMPLATE_PATH" ]]; then
  echo "[entrypoint] template not found: $TEMPLATE_PATH" >&2
  exit 1
fi
render_config "$TEMPLATE_PATH" "$CONFIG_PATH"

echo "[entrypoint] starting app command: $APP_CMD"
bash -lc "$APP_CMD" &
APP_PID=$!

echo "[entrypoint] starting tcpao-proxy terminator with config $CONFIG_PATH"
tcpao-proxy --mode terminator --config "$CONFIG_PATH" &
PROXY_PID=$!

trap 'shutdown 143' INT TERM

set +e
wait -n "$APP_PID" "$PROXY_PID"
status=$?
set -e

echo "[entrypoint] one process exited; stopping both (status=${status})" >&2
shutdown "$status"
