#!/usr/bin/env bash
set -euo pipefail

TOPOLOGY="${TOPOLOGY:-deploy/containerlab/tcpao-bmp.clab.yml}"
INIT_NODE="${INIT_NODE:-gobgp-initiator-with-tcpao-sidecar}"
TERM_NODE="${TERM_NODE:-gobmp-terminator-with-tcpao-sidecar}"
LISTEN_PLAIN="${LISTEN_PLAIN:-127.0.0.1:5000}"
LISTEN_AO_PORT="${LISTEN_AO_PORT:-1790}"
FORWARD_PLAIN="${FORWARD_PLAIN:-127.0.0.1:11019}"
MAX_WAIT_SECS="${MAX_WAIT_SECS:-60}"
ROUTE_PREFIX="${ROUTE_PREFIX:-203.0.113.0/24}"
ROUTE_NEXTHOP="${ROUTE_NEXTHOP:-192.0.2.1}"
GOBGP_CONFIG_PATH="${GOBGP_CONFIG_PATH:-/tmp/gobgp-bmp-route-validation.conf}"
GOBGP_LOG_PATH="${GOBGP_LOG_PATH:-/tmp/tcpao-bgp-route-gobgp.log}"
GOBMP_LOG_PATH="${GOBMP_LOG_PATH:-/tmp/tcpao-bgp-route-gobmp.log}"
GOBMP_DUMP_PATH="${GOBMP_DUMP_PATH:-/tmp/tcpao-bgp-route-messages.json}"
JQ_INSTALL_LOG="${JQ_INSTALL_LOG:-/tmp/tcpao-bgp-route-jq-install.log}"
USE_JQ=0

step() {
  echo "[step] $*"
}

info() {
  echo "[info] $*"
}

ok() {
  echo "[ok] $*"
}

warn() {
  echo "[warn] $*"
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

run_in_container() {
  local container="$1"
  local cmd="$2"
  docker exec "$container" bash -lc "$cmd"
}

run_host_privileged() {
  local cmd="$1"
  if [[ "$(id -u)" -eq 0 ]]; then
    bash -lc "$cmd"
    return $?
  fi
  if command -v sudo >/dev/null 2>&1; then
    sudo -E bash -lc "$cmd"
    return $?
  fi
  return 1
}

ensure_jq_or_fallback() {
  local install_cmd=""

  if command -v jq >/dev/null 2>&1; then
    USE_JQ=1
    info "jq available; route evidence will be pretty-printed"
    return 0
  fi

  step "jq not found on host; attempting auto-install"
  if command -v dnf >/dev/null 2>&1; then
    install_cmd="dnf install -y jq"
  elif command -v yum >/dev/null 2>&1; then
    install_cmd="yum install -y jq"
  elif command -v apt-get >/dev/null 2>&1; then
    install_cmd="apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y jq"
  elif command -v microdnf >/dev/null 2>&1; then
    install_cmd="microdnf install -y jq"
  fi

  if [[ -z "$install_cmd" ]]; then
    warn "no supported package manager found; falling back to non-pretty route evidence output"
    USE_JQ=0
    return 0
  fi

  if run_host_privileged "$install_cmd" >"$JQ_INSTALL_LOG" 2>&1 && command -v jq >/dev/null 2>&1; then
    USE_JQ=1
    ok "installed jq; route evidence will be pretty-printed"
    return 0
  fi

  warn "jq auto-install failed; falling back to non-pretty route evidence output"
  warn "jq install diagnostics: $JQ_INSTALL_LOG"
  USE_JQ=0
}

dump_contains_route() {
  local dump_content="$1"
  local route="$2"
  local route_ip="$3"
  local route_len="$4"
  local encoded=""
  local decoded=""

  if [[ -z "$dump_content" ]]; then
    return 1
  fi

  if printf '%s\n' "$dump_content" | grep -q "$route"; then
    return 0
  fi

  if printf '%s\n' "$dump_content" | grep -q "\"prefix\":\"$route_ip\"" &&
    printf '%s\n' "$dump_content" | grep -q "\"prefix_len\":$route_len"; then
    return 0
  fi

  if ! command -v base64 >/dev/null 2>&1; then
    return 1
  fi

  encoded="$(printf '%s\n' "$dump_content" | sed -n -E 's/.*"value":"([^"]+)".*/\1/p')"
  [[ -n "$encoded" ]] || return 1

  decoded="$(
    while IFS= read -r v; do
      [[ -n "$v" ]] || continue
      printf '%s' "$v" | base64 -d 2>/dev/null || true
      printf '\n'
    done <<< "$encoded"
  )"

  if printf '%s\n' "$decoded" | grep -q "$route"; then
    return 0
  fi
  if printf '%s\n' "$decoded" | grep -q "\"prefix\":\"$route_ip\"" &&
    printf '%s\n' "$decoded" | grep -q "\"prefix_len\":$route_len"; then
    return 0
  fi

  return 1
}

run_first_success() {
  local container="$1"
  local desc="$2"
  shift 2
  local cmd

  for cmd in "$@"; do
    if run_in_container "$container" "$cmd" >/dev/null 2>&1; then
      ok "$desc succeeded with: $cmd"
      return 0
    fi
  done

  echo "[diag] failed command set for $desc:"
  for cmd in "$@"; do
    echo "  - $cmd"
  done
  fail "$desc failed with all attempted commands"
}

start_gobmp_backend_for_route_validation() {
  local term_container="$1"
  local forward_plain="$2"
  local forward_port="$3"
  local backend_bin=""
  local backend_cmd=""
  local waited=0

  if is_listening_on_port "$term_container" "$forward_port"; then
    ok "goBMP backend already listening on :$forward_port"
    return 0
  fi

  if run_in_container "$term_container" "test -x /gobmp"; then
    backend_bin="/gobmp"
  elif run_in_container "$term_container" "command -v gobmp >/dev/null 2>&1"; then
    backend_bin="gobmp"
  else
    fail "gobmp binary not found in $term_container"
  fi

  if run_in_container "$term_container" "$backend_bin --help 2>&1 | grep -q -- '--listen'"; then
    backend_cmd="$backend_bin --listen $forward_plain"
  elif run_in_container "$term_container" "$backend_bin --help 2>&1 | grep -q -- '--source-port'"; then
    backend_cmd="$backend_bin --source-port $forward_port"
  else
    fail "gobmp in $term_container does not support --listen or --source-port"
  fi

  if run_in_container "$term_container" "$backend_bin --help 2>&1 | grep -q -- '--dump'"; then
    backend_cmd="$backend_cmd --dump file"
  fi
  if run_in_container "$term_container" "$backend_bin --help 2>&1 | grep -q -- '--msg-file'"; then
    backend_cmd="$backend_cmd --msg-file $GOBMP_DUMP_PATH"
  fi

  step "starting goBMP backend for route validation: $backend_cmd"
  run_in_container "$term_container" "rm -f $GOBMP_DUMP_PATH $GOBMP_LOG_PATH"
  run_in_container "$term_container" "nohup $backend_cmd >$GOBMP_LOG_PATH 2>&1 &"

  while (( waited < MAX_WAIT_SECS )); do
    if is_listening_on_port "$term_container" "$forward_port"; then
      ok "goBMP backend is listening on :$forward_port"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  step "goBMP backend failed to open :$forward_port; dumping diagnostics"
  run_in_container "$term_container" "ss -ltnp || true"
  run_in_container "$term_container" "ps -ef | grep -E '[g]obmp|tcpao-proxy' || true"
  run_in_container "$term_container" "tail -n 120 $GOBMP_LOG_PATH 2>/dev/null || true"
  fail "goBMP backend startup failed"
}

start_gobgp_for_bmp_export() {
  local init_container="$1"
  local bmp_host="$2"
  local bmp_port="$3"
  local cfg="$4"
  local log="$5"
  local waited=0

  step "rendering goBGP config for BMP export to ${bmp_host}:${bmp_port}"
  run_in_container "$init_container" "cat > $cfg <<'EOF'
[global.config]
  as = 65000
  router-id = \"192.0.2.1\"

[[bmp-servers]]
[bmp-servers.config]
  address = \"${bmp_host}\"
  port = ${bmp_port}
  route-monitoring-policy = \"all\"
EOF"

  run_in_container "$init_container" "pkill -f '^gobgpd' || true"
  run_in_container "$init_container" "rm -f $log"

  step "starting goBGP daemon with $cfg"
  if ! run_in_container "$init_container" "command -v gobgpd >/dev/null 2>&1"; then
    fail "gobgpd binary not found in $init_container"
  fi
  if ! run_in_container "$init_container" "command -v gobgp >/dev/null 2>&1"; then
    fail "gobgp binary not found in $init_container"
  fi

  run_in_container "$init_container" "nohup gobgpd -f $cfg >$log 2>&1 &"

  while (( waited < MAX_WAIT_SECS )); do
    if run_in_container "$init_container" "pgrep -f '^gobgpd' >/dev/null 2>&1"; then
      ok "goBGP daemon is running"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  run_in_container "$init_container" "tail -n 120 $log 2>/dev/null || true"
  fail "goBGP daemon failed to start"
}

inject_route_in_gobgp() {
  local init_container="$1"
  local route="$2"
  local nexthop="$3"

  step "injecting route in goBGP: $route"
  run_first_success "$init_container" "gobgp route injection" \
    "gobgp global rib add $route" \
    "gobgp global rib add $route nexthop $nexthop" \
    "gobgp global rib add $route nexthop 0.0.0.0" \
    "gobgp global rib add -a ipv4 $route nexthop $nexthop" \
    "gobgp global rib -a ipv4 add $route nexthop $nexthop"
}

verify_route_exists_in_gobgp() {
  local init_container="$1"
  local route="$2"

  run_first_success "$init_container" "gobgp route verification" \
    "gobgp global rib | grep -q '$route'" \
    "gobgp global rib -a ipv4 | grep -q '$route'" \
    "gobgp global rib -a ipv4 2>/dev/null | grep -q '$route'"
}

wait_for_route_in_gobmp_dump() {
  local term_container="$1"
  local route="$2"
  local route_ip="${route%/*}"
  local route_len="${route#*/}"
  local dump_content=""
  local waited=0

  if [[ "$route_ip" == "$route" || -z "$route_len" ]]; then
    fail "ROUTE_PREFIX must be CIDR (for example 203.0.113.0/24), got: $route"
  fi

  step "waiting for goBMP to receive route: $route"
  while (( waited < MAX_WAIT_SECS )); do
    dump_content="$(run_in_container "$term_container" "cat $GOBMP_DUMP_PATH 2>/dev/null || true")"
    if dump_contains_route "$dump_content" "$route" "$route_ip" "$route_len"; then
      ok "goBMP dump file contains route: $route"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  step "route not found in goBMP dump; printing diagnostics"
  run_in_container "$term_container" "ls -l $GOBMP_DUMP_PATH 2>/dev/null || true"
  run_in_container "$term_container" "tail -n 120 $GOBMP_DUMP_PATH 2>/dev/null || true"
  run_in_container "$term_container" "tail -n 120 $GOBMP_LOG_PATH 2>/dev/null || true"
  fail "goBMP did not receive route $route within ${MAX_WAIT_SECS}s"
}

wait_for_connection_closed_stats() {
  local init_container="$1"
  local term_container="$2"
  local waited=0

  run_in_container "$init_container" "pkill -f '^gobgpd' || true"

  while (( waited < MAX_WAIT_SECS )); do
    if docker logs "$init_container" 2>&1 | grep -q "connection closed" &&
      docker logs "$term_container" 2>&1 | grep -q "connection closed"; then
      ok "connection-closed stats are present on both proxies"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  step "connection-closed stats not found within ${MAX_WAIT_SECS}s; dumping recent logs"
  docker logs "$init_container" 2>&1 | tail -n 120 || true
  docker logs "$term_container" 2>&1 | tail -n 120 || true
  fail "timed out waiting for connection-closed stats on both proxies"
}

show_gobmp_route_evidence_pretty() {
  local term_container="$1"
  local route="$2"
  local route_ip="${route%/*}"
  local route_len="${route#*/}"

  step "showing goBMP route evidence (pretty via jq)"
  run_in_container "$term_container" "cat $GOBMP_DUMP_PATH 2>/dev/null || true" \
    | jq -R -s --arg prefix "$route_ip" --argjson plen "$route_len" '
        split("\n")
        | map(select(length > 0) | (try fromjson catch empty))
        | map(
            . as $outer
            | (try ($outer.value | @base64d | fromjson) catch empty) as $decoded
            | select($decoded.prefix == $prefix and $decoded.prefix_len == $plen)
            | {
                type: $outer.type,
                decoded: $decoded
              }
          )
      ' || true
}

show_gobmp_route_evidence_fallback() {
  local term_container="$1"
  local route="$2"
  local route_ip="${route%/*}"
  local route_len="${route#*/}"

  step "showing goBMP route evidence (fallback, non-pretty)"
  run_in_container "$term_container" "tail -n 40 $GOBMP_DUMP_PATH 2>/dev/null || true"
  if command -v base64 >/dev/null 2>&1; then
    run_in_container "$term_container" "cat $GOBMP_DUMP_PATH 2>/dev/null || true" \
      | sed -n -E 's/.*\"value\":\"([^\"]+)\".*/\1/p' \
      | while IFS= read -r v; do
          [[ -n "$v" ]] || continue
          printf '%s' "$v" | base64 -d 2>/dev/null || true
          printf '\n'
        done \
      | grep -E "\"prefix\":\"${route_ip}\"|\"prefix_len\":${route_len}" \
      | tail -n 20 || true
  fi
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

extract_latest_counter() {
  local logs="$1"
  local field="$2"
  local who="$3"
  local line=""
  local value=""

  line="$(printf '%s\n' "$logs" | grep -E 'connection closed' | tail -n 1 || true)"
  [[ -n "$line" ]] || fail "$who has no connection-closed log entry to parse $field"

  value="$(printf '%s\n' "$line" | sed -n -E "s/.*\"$field\":([0-9]+).*/\\1/p")"
  if [[ -z "$value" ]]; then
    value="$(printf '%s\n' "$line" | sed -n -E "s/.*$field[=: ]+([0-9]+).*/\\1/p")"
  fi
  [[ -n "$value" ]] || fail "unable to parse $field from $who connection-closed log: $line"

  printf '%s' "$value"
}

main() {
  require_cmd containerlab
  require_cmd docker
  require_cmd awk
  require_cmd grep
  require_cmd bash
  ensure_jq_or_fallback

  [[ -f "$TOPOLOGY" ]] || fail "topology file not found: $TOPOLOGY"
  if ! docker ps >/dev/null 2>&1; then
    fail "cannot access docker daemon; run with sufficient privileges (for example: sudo -E make test-validation-tcpao-proxy-bgp-route)"
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

  info "route under test: $ROUTE_PREFIX"
  info "goBMP dump path: $GOBMP_DUMP_PATH"

  step "deploying $TOPOLOGY with --reconfigure"
  containerlab deploy -t "$TOPOLOGY" --reconfigure

  step "inspecting deployed topology"
  containerlab inspect -t "$TOPOLOGY"

  wait_for_listen_port "$init_container" "$plain_port" "goBGP-side proxy"
  wait_for_listen_port "$term_container" "$LISTEN_AO_PORT" "goBMP-side proxy"

  start_gobmp_backend_for_route_validation "$term_container" "$FORWARD_PLAIN" "$forward_port"
  start_gobgp_for_bmp_export "$init_container" "$plain_host" "$plain_port" "$GOBGP_CONFIG_PATH" "$GOBGP_LOG_PATH"
  inject_route_in_gobgp "$init_container" "$ROUTE_PREFIX" "$ROUTE_NEXTHOP"
  verify_route_exists_in_gobgp "$init_container" "$ROUTE_PREFIX"
  wait_for_route_in_gobmp_dump "$term_container" "$ROUTE_PREFIX"
  wait_for_connection_closed_stats "$init_container" "$term_container"

  local init_logs
  local term_logs
  init_logs="$(docker logs "$init_container" 2>&1 || true)"
  term_logs="$(docker logs "$term_container" 2>&1 || true)"

  assert_log_contains "$init_logs" "applied outbound tcp-ao policy" "AO outbound policy applied on goBGP-side proxy"
  assert_log_contains "$term_logs" "configured tcp-ao policies on listener" "AO listener policy configured on goBMP-side proxy"
  assert_log_absent "$init_logs" "failed to apply outbound AO policy" "outbound AO failure"
  assert_log_absent "$term_logs" "inbound AO verification failed" "inbound AO verification failure"
  assert_log_absent "$init_logs"$'\n'"$term_logs" "tcp-ao test bypass enabled" "test bypass marker"

  local init_bytes_up
  local term_bytes_up
  init_bytes_up="$(extract_latest_counter "$init_logs" "bytes_up" "goBGP-side proxy")"
  term_bytes_up="$(extract_latest_counter "$term_logs" "bytes_up" "goBMP-side proxy")"
  (( init_bytes_up > 0 )) || fail "goBGP-side proxy has no forwarded bytes (bytes_up=$init_bytes_up)"
  (( term_bytes_up > 0 )) || fail "goBMP-side proxy has no forwarded bytes (bytes_up=$term_bytes_up)"
  ok "forwarded bytes observed (goBGP bytes_up=$init_bytes_up, goBMP bytes_up=$term_bytes_up)"

  if (( USE_JQ == 1 )); then
    show_gobmp_route_evidence_pretty "$term_container" "$ROUTE_PREFIX"
  else
    show_gobmp_route_evidence_fallback "$term_container" "$ROUTE_PREFIX"
  fi

  ok "test-validation-tcpao-proxy-bgp-route passed"
}

main "$@"
