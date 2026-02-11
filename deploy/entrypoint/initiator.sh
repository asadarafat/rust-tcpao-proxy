#!/usr/bin/env bash
set -euo pipefail

TEMPLATE_PATH="${TEMPLATE_PATH:-/etc/tcpao-proxy/initiator.toml.tmpl}"
CONFIG_PATH="${CONFIG_PATH:-/etc/tcpao-proxy/config.toml}"

LISTEN_PLAIN="${LISTEN_PLAIN:-127.0.0.1:5000}"
REMOTE_AO="${REMOTE_AO:-}"
PEER_IP="${PEER_IP:-}"
PEER_PORT="${PEER_PORT:-}"
KEY_ID="${KEY_ID:-1}"
APP_CMD="${APP_CMD:-}"

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

infer_peer_values() {
  if [[ -z "$REMOTE_AO" ]]; then
    echo "[entrypoint] REMOTE_AO must be set (example: 10.10.10.2:1790)" >&2
    exit 1
  fi

  if [[ -z "$PEER_IP" ]]; then
    PEER_IP="${REMOTE_AO%%:*}"
  fi
  if [[ -z "$PEER_PORT" ]]; then
    PEER_PORT="${REMOTE_AO##*:}"
  fi
}

render_config() {
  local template="$1"
  local output="$2"

  local listen_plain remote_ao peer_ip peer_port key_id
  listen_plain="$(escape_awk_replacement "$LISTEN_PLAIN")"
  remote_ao="$(escape_awk_replacement "$REMOTE_AO")"
  peer_ip="$(escape_awk_replacement "$PEER_IP")"
  peer_port="$(escape_awk_replacement "$PEER_PORT")"
  key_id="$(escape_awk_replacement "$KEY_ID")"

  awk \
    -v LISTEN_PLAIN="$listen_plain" \
    -v REMOTE_AO="$remote_ao" \
    -v PEER_IP="$peer_ip" \
    -v PEER_PORT="$peer_port" \
    -v KEY_ID="$key_id" '
      {
        gsub(/\$\{LISTEN_PLAIN\}/, LISTEN_PLAIN);
        gsub(/\$\{REMOTE_AO\}/, REMOTE_AO);
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
infer_peer_values

mkdir -p "$(dirname "$CONFIG_PATH")"
if [[ ! -f "$TEMPLATE_PATH" ]]; then
  echo "[entrypoint] template not found: $TEMPLATE_PATH" >&2
  exit 1
fi
render_config "$TEMPLATE_PATH" "$CONFIG_PATH"

APP_STARTED=0
if [[ -n "$APP_CMD" ]]; then
  echo "[entrypoint] starting app command: $APP_CMD"
  bash -lc "$APP_CMD" &
  APP_PID=$!
  APP_STARTED=1
else
  echo "[entrypoint] APP_CMD is empty; starting tcpao-proxy only"
fi

echo "[entrypoint] starting tcpao-proxy initiator with config $CONFIG_PATH"
tcpao-proxy --mode initiator --config "$CONFIG_PATH" &
PROXY_PID=$!

trap 'shutdown 143' INT TERM

set +e
if [[ "$APP_STARTED" -eq 1 ]]; then
  wait -n "$APP_PID" "$PROXY_PID"
else
  wait "$PROXY_PID"
fi
status=$?
set -e

echo "[entrypoint] one process exited; stopping both (status=${status})" >&2
shutdown "$status"
