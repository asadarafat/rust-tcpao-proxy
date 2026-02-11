#!/usr/bin/env bash
set -euo pipefail

TOPOLOGY="${TOPOLOGY:-deploy/containerlab/tcpao-bmp.clab.yml}"
INIT_NODE="${INIT_NODE:-gobgp-initiator-with-tcpao-sidecar}"
TERM_NODE="${TERM_NODE:-gobmp-terminator-with-tcpao-sidecar}"
LISTEN_PLAIN="${LISTEN_PLAIN:-127.0.0.1:5000}"
LISTEN_AO_PORT="${LISTEN_AO_PORT:-1790}"
FORWARD_PLAIN="${FORWARD_PLAIN:-127.0.0.1:11019}"
MAX_WAIT_SECS="${MAX_WAIT_SECS:-60}"

step() {
  echo "[step] $*"
}

ok() {
  echo "[ok] $*"
}

fail() {
  echo "[fail] $*" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1 || fail "required command not found: $cmd"
}

is_listening_on_port() {
  local container="$1"
  local port="$2"
  docker exec "$container" ss -ltnp 2>/dev/null | grep -Eq ":${port}[[:space:]]"
}

wait_for_listen_port() {
  local container="$1"
  local port="$2"
  local label="$3"
  local waited=0

  while (( waited < MAX_WAIT_SECS )); do
    if is_listening_on_port "$container" "$port"; then
      ok "$label is listening on :$port"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  docker exec "$container" ss -ltnp || true
  fail "timed out waiting for $label to listen on :$port"
}

start_terminator_backend_if_needed() {
  local term_container="$1"
  local forward_plain="$2"
  local forward_port="$3"
  local backend_log="/tmp/tcpao-validation-backend.log"
  local backend_bin=""
  local backend_cmd=""
  local waited=0

  if is_listening_on_port "$term_container" "$forward_port"; then
    ok "terminator backend already listening on :$forward_port"
    return 0
  fi

  step "no backend listener on $forward_plain; starting temporary /gobmp backend"
  if docker exec "$term_container" bash -lc "test -x /gobmp"; then
    backend_bin="/gobmp"
  elif docker exec "$term_container" bash -lc "command -v gobmp >/dev/null 2>&1"; then
    backend_bin="gobmp"
  else
    fail "forward backend is not listening and gobmp binary was not found in $term_container; set APP_CMD in topology or install a listener on $forward_plain"
  fi

  if docker exec "$term_container" bash -lc "$backend_bin --help 2>&1 | grep -q -- '--listen'"; then
    backend_cmd="$backend_bin --listen $forward_plain"
  elif docker exec "$term_container" bash -lc "$backend_bin --help 2>&1 | grep -q -- '--source-port'"; then
    backend_cmd="$backend_bin --source-port $forward_port"
  else
    fail "gobmp in $term_container does not support --listen or --source-port; set APP_CMD manually for a backend on $forward_plain"
  fi

  step "starting temporary gobmp backend: $backend_cmd"
  docker exec "$term_container" bash -lc "nohup $backend_cmd >$backend_log 2>&1 &"

  while (( waited < MAX_WAIT_SECS )); do
    if is_listening_on_port "$term_container" "$forward_port"; then
      ok "terminator backend is listening on :$forward_port"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  step "terminator backend failed to open :$forward_port; dumping diagnostics"
  docker exec "$term_container" bash -lc "ss -ltnp || true"
  docker exec "$term_container" bash -lc "ps -ef | grep -E '[g]obmp|tcpao-proxy' || true"
  docker exec "$term_container" bash -lc "tail -n 120 $backend_log 2>/dev/null || true"
  fail "temporary backend did not start on $forward_plain; set APP_CMD in topology to a working backend command"
}

inject_test_payload() {
  local init_container="$1"
  local host="$2"
  local port="$3"
  local attempts=10
  local i

  for ((i = 1; i <= attempts; i++)); do
    if docker exec "$init_container" bash -lc "exec 3<>/dev/tcp/$host/$port; echo tcpao-functional-test-$i >&3; sleep 1; exec 3<&-; exec 3>&-"; then
      ok "injected payload via $host:$port (attempt $i)"
      return 0
    fi
    sleep 1
  done

  fail "unable to inject payload into initiator listener at $host:$port after $attempts attempts"
}

assert_log_contains() {
  local logs="$1"
  local pattern="$2"
  local desc="$3"

  printf '%s\n' "$logs" | grep -q "$pattern" || fail "missing log evidence: $desc"
  ok "$desc"
}

assert_log_absent() {
  local logs="$1"
  local pattern="$2"
  local desc="$3"

  if printf '%s\n' "$logs" | grep -q "$pattern"; then
    fail "unexpected log entry found: $desc"
  fi
  ok "$desc not present"
}

main() {
  require_cmd containerlab
  require_cmd docker
  require_cmd awk
  require_cmd grep
  require_cmd bash

  [[ -f "$TOPOLOGY" ]] || fail "topology file not found: $TOPOLOGY"

  if ! docker ps >/dev/null 2>&1; then
    fail "cannot access docker daemon; run with sufficient privileges (for example: sudo -E make test-validation-tcpao-proxy)"
  fi

  local lab_name
  lab_name="$(awk '/^name:[[:space:]]*/ {print $2; exit}' "$TOPOLOGY")"
  [[ -n "$lab_name" ]] || fail "unable to parse lab name from topology: $TOPOLOGY"

  local init_container="clab-${lab_name}-${INIT_NODE}"
  local term_container="clab-${lab_name}-${TERM_NODE}"
  local plain_host="${LISTEN_PLAIN%:*}"
  local plain_port="${LISTEN_PLAIN##*:}"
  local forward_port="${FORWARD_PLAIN##*:}"

  [[ "$plain_host" != "$LISTEN_PLAIN" ]] || fail "LISTEN_PLAIN must be host:port, got: $LISTEN_PLAIN"
  [[ "$plain_port" != "$LISTEN_PLAIN" ]] || fail "LISTEN_PLAIN must include a port, got: $LISTEN_PLAIN"
  [[ "$forward_port" != "$FORWARD_PLAIN" ]] || fail "FORWARD_PLAIN must include a port, got: $FORWARD_PLAIN"

  step "deploying $TOPOLOGY with --reconfigure"
  containerlab deploy -t "$TOPOLOGY" --reconfigure

  step "inspecting deployed topology"
  containerlab inspect -t "$TOPOLOGY"

  wait_for_listen_port "$init_container" "$plain_port" "initiator proxy"
  wait_for_listen_port "$term_container" "$LISTEN_AO_PORT" "terminator proxy"
  start_terminator_backend_if_needed "$term_container" "$FORWARD_PLAIN" "$forward_port"

  inject_test_payload "$init_container" "$plain_host" "$plain_port"
  sleep 1

  local init_logs
  local term_logs
  init_logs="$(docker logs "$init_container" 2>&1 || true)"
  term_logs="$(docker logs "$term_container" 2>&1 || true)"

  assert_log_contains "$init_logs" "applied outbound tcp-ao policy" "initiator applied outbound AO policy"
  assert_log_contains "$term_logs" "configured tcp-ao policies on listener" "terminator configured listener AO policy"

  assert_log_absent "$init_logs" "failed to apply outbound AO policy" "initiator outbound AO failure"
  assert_log_absent "$term_logs" "inbound AO verification failed" "terminator inbound AO verification failure"
  assert_log_absent "$init_logs"$'\n'"$term_logs" "tcp-ao test bypass enabled" "test bypass marker"

  printf '%s\n' "$init_logs" | grep -E 'connection closed' | grep -Eq 'bytes_up[=: ]*[1-9][0-9]*|"bytes_up":[1-9][0-9]*' \
    || fail "initiator has no connection-closed entry with bytes_up>0"
  ok "initiator recorded forwarded bytes"

  printf '%s\n' "$term_logs" | grep -E 'connection closed' | grep -Eq 'bytes_up[=: ]*[1-9][0-9]*|"bytes_up":[1-9][0-9]*' \
    || fail "terminator has no connection-closed entry with bytes_up>0"
  ok "terminator recorded forwarded bytes"

  ok "test-validation-tcpao-proxy passed"
}

main "$@"
