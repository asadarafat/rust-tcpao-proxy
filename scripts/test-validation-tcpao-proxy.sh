#!/usr/bin/env bash
set -euo pipefail

TOPOLOGY="${TOPOLOGY:-deploy/containerlab/tcpao-bmp.clab.yml}"
INIT_NODE="${INIT_NODE:-gobgp-initiator-with-tcpao-sidecar}"
TERM_NODE="${TERM_NODE:-gobmp-terminator-with-tcpao-sidecar}"
LISTEN_PLAIN="${LISTEN_PLAIN:-127.0.0.1:5000}"
LISTEN_AO_PORT="${LISTEN_AO_PORT:-1790}"
FORWARD_PLAIN="${FORWARD_PLAIN:-127.0.0.1:11019}"
MAX_WAIT_SECS="${MAX_WAIT_SECS:-60}"
REQUIRE_BIDIRECTIONAL_TRAFFIC="${REQUIRE_BIDIRECTIONAL_TRAFFIC:-0}"
BACKEND_MODE="${BACKEND_MODE:-auto}"

DIRECTION_GOBGP_TO_GOBMP="from-goBGP-to-goBMP"
DIRECTION_GOBMP_TO_GOBGP="from-goBMP-to-goBGP"

step() {
  echo "[step] $*"
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

is_true() {
  local v="${1:-}"
  case "${v,,}" in
    1|true|yes|y|on)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
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
  local backend_mode="$4"
  local backend_log="/tmp/tcpao-validation-backend.log"
  local backend_bin=""
  local backend_cmd=""
  local apt_log="/tmp/tcpao-validation-apt.log"
  local waited=0

  if is_listening_on_port "$term_container" "$forward_port"; then
    ok "goBMP backend already listening on :$forward_port"
    if [[ "$backend_mode" == "echo" ]]; then
      warn "backend mode is echo but an existing listener is already running on :$forward_port; script will not replace it"
    fi
    return 0
  fi

  if [[ "$backend_mode" == "echo" ]]; then
    step "no backend listener on $forward_plain; starting temporary echo backend"
    if docker exec "$term_container" bash -lc "command -v socat >/dev/null 2>&1"; then
      backend_cmd="socat TCP-LISTEN:${forward_port},bind=127.0.0.1,reuseaddr,fork EXEC:/bin/cat"
    elif docker exec "$term_container" bash -lc "command -v perl >/dev/null 2>&1"; then
      backend_cmd="perl -MIO::Socket::INET -e 'my \$p=shift; my \$s=IO::Socket::INET->new(LocalAddr=>\"127.0.0.1\",LocalPort=>\$p,Proto=>\"tcp\",Listen=>10,Reuse=>1) or die \$!; while(my \$c=\$s->accept()){ while(sysread(\$c,my \$buf,65535)){ syswrite(\$c,\$buf); } close(\$c); }' ${forward_port}"
    elif docker exec "$term_container" bash -lc "command -v apt-get >/dev/null 2>&1"; then
      step "echo backend requires socat/perl; attempting apt-get install socat"
      docker exec "$term_container" bash -lc "apt-get update >$apt_log 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends socat >>$apt_log 2>&1" \
        || {
          docker exec "$term_container" bash -lc "tail -n 120 $apt_log 2>/dev/null || true"
          fail "failed to install socat for echo backend in $term_container"
        }
      backend_cmd="socat TCP-LISTEN:${forward_port},bind=127.0.0.1,reuseaddr,fork EXEC:/bin/cat"
    else
      fail "echo backend mode requested but no socat/perl/apt-get available in $term_container"
    fi
  else
    step "no goBMP backend listener on $forward_plain; starting temporary gobmp backend"
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

    # Some gobmp builds default to Kafka output when dump mode is unset.
    # Force console dump when supported so the backend can start without Kafka config.
    if docker exec "$term_container" bash -lc "$backend_bin --help 2>&1 | grep -q -- '--dump'"; then
      backend_cmd="$backend_cmd --dump console"
    fi
  fi

  step "starting temporary backend ($backend_mode): $backend_cmd"
  docker exec "$term_container" bash -lc "nohup $backend_cmd >$backend_log 2>&1 &"

  while (( waited < MAX_WAIT_SECS )); do
    if is_listening_on_port "$term_container" "$forward_port"; then
      ok "goBMP backend is listening on :$forward_port"
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done

  step "backend ($backend_mode) failed to open :$forward_port; dumping diagnostics"
  docker exec "$term_container" bash -lc "ss -ltnp || true"
  docker exec "$term_container" bash -lc "ps -ef | grep -E '[g]obmp|tcpao-proxy' || true"
  docker exec "$term_container" bash -lc "tail -n 120 $backend_log 2>/dev/null || true"
  fail "temporary backend did not start on $forward_plain; set APP_CMD in topology to a working backend command"
}

inject_test_payload() {
  local init_container="$1"
  local host="$2"
  local port="$3"
  local require_bidirectional="${4:-0}"
  local attempts=10
  local i

  for ((i = 1; i <= attempts; i++)); do
    if is_true "$require_bidirectional"; then
      if docker exec "$init_container" bash -lc "payload=tcpao-functional-test-$i; exec 3<>/dev/tcp/$host/$port; echo \"\$payload\" >&3; IFS= read -r -t 5 reply <&3; test \"\$reply\" = \"\$payload\"; exec 3<&-; exec 3>&-"; then
        ok "$DIRECTION_GOBGP_TO_GOBMP payload injected and echoed via $host:$port (attempt $i)"
        return 0
      fi
    else
      if docker exec "$init_container" bash -lc "exec 3<>/dev/tcp/$host/$port; echo tcpao-functional-test-$i >&3; sleep 1; exec 3<&-; exec 3>&-"; then
        ok "$DIRECTION_GOBGP_TO_GOBMP payload injected via $host:$port (attempt $i)"
        return 0
      fi
    fi
    sleep 1
  done

  if is_true "$require_bidirectional"; then
    fail "unable to inject+echo $DIRECTION_GOBGP_TO_GOBMP payload via listener at $host:$port after $attempts attempts"
  fi
  fail "unable to inject $DIRECTION_GOBGP_TO_GOBMP payload into listener at $host:$port after $attempts attempts"
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

dump_runtime_configs() {
  local init_container="$1"
  local term_container="$2"

  step "spitting out runtime configs for goBGP/goBMP and tcpao-proxy"

  echo "[config][$INIT_NODE] /etc/tcpao-proxy/config.toml"
  docker exec "$init_container" bash -lc 'cat /etc/tcpao-proxy/config.toml 2>/dev/null || echo "<missing>"'

  echo "[config][$INIT_NODE] app config candidates"
  docker exec "$init_container" bash -lc '
for f in \
  /etc/gobgp/gobgp.conf \
  /etc/gobgp/gobgp-bmp-test.conf \
  /etc/stunnel/stunnel.conf \
  /etc/tcpao-proxy/initiator.toml.tmpl
do
  if [[ -f "$f" ]]; then
    echo "--- $f ---"
    cat "$f"
  fi
done
'

  echo "[config][$TERM_NODE] /etc/tcpao-proxy/config.toml"
  docker exec "$term_container" bash -lc 'cat /etc/tcpao-proxy/config.toml 2>/dev/null || echo "<missing>"'

  echo "[config][$TERM_NODE] app config candidates"
  docker exec "$term_container" bash -lc '
for f in \
  /etc/gobmp/gobmp.yaml \
  /root/.gobmp.yaml \
  /etc/tcpao-proxy/terminator.toml.tmpl
do
  if [[ -f "$f" ]]; then
    echo "--- $f ---"
    cat "$f"
  fi
done
'

  echo "[config][$INIT_NODE] processes"
  docker exec "$init_container" bash -lc "ps -ef | grep -E '[t]cpao-proxy|[g]obgp|[s]tunnel' || true"

  echo "[config][$TERM_NODE] processes"
  docker exec "$term_container" bash -lc "ps -ef | grep -E '[t]cpao-proxy|[g]obmp' || true"
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

wait_for_connection_closed_stats() {
  local init_container="$1"
  local term_container="$2"
  local waited=0

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
  local backend_mode="$BACKEND_MODE"

  [[ "$plain_host" != "$LISTEN_PLAIN" ]] || fail "LISTEN_PLAIN must be host:port, got: $LISTEN_PLAIN"
  [[ "$plain_port" != "$LISTEN_PLAIN" ]] || fail "LISTEN_PLAIN must include a port, got: $LISTEN_PLAIN"
  [[ "$forward_port" != "$FORWARD_PLAIN" ]] || fail "FORWARD_PLAIN must include a port, got: $FORWARD_PLAIN"

  if [[ "$backend_mode" == "auto" ]]; then
    if is_true "$REQUIRE_BIDIRECTIONAL_TRAFFIC"; then
      backend_mode="echo"
    else
      backend_mode="gobmp"
    fi
  fi
  case "$backend_mode" in
    gobmp|echo) ;;
    *)
      fail "BACKEND_MODE must be one of: auto, gobmp, echo (got: $BACKEND_MODE)"
      ;;
  esac
  step "selected backend mode: $backend_mode"

  step "deploying $TOPOLOGY with --reconfigure"
  containerlab deploy -t "$TOPOLOGY" --reconfigure

  step "inspecting deployed topology"
  containerlab inspect -t "$TOPOLOGY"

  wait_for_listen_port "$init_container" "$plain_port" "goBGP-side proxy"
  wait_for_listen_port "$term_container" "$LISTEN_AO_PORT" "goBMP-side proxy"
  start_terminator_backend_if_needed "$term_container" "$FORWARD_PLAIN" "$forward_port" "$backend_mode"
  dump_runtime_configs "$init_container" "$term_container"

  inject_test_payload "$init_container" "$plain_host" "$plain_port" "$REQUIRE_BIDIRECTIONAL_TRAFFIC"
  wait_for_connection_closed_stats "$init_container" "$term_container"

  local init_logs
  local term_logs
  init_logs="$(docker logs "$init_container" 2>&1 || true)"
  term_logs="$(docker logs "$term_container" 2>&1 || true)"

  assert_log_contains "$init_logs" "applied outbound tcp-ao policy" "$DIRECTION_GOBGP_TO_GOBMP AO outbound policy applied"
  assert_log_contains "$term_logs" "configured tcp-ao policies on listener" "$DIRECTION_GOBGP_TO_GOBMP AO listener policy configured"

  assert_log_absent "$init_logs" "failed to apply outbound AO policy" "$DIRECTION_GOBGP_TO_GOBMP outbound AO failure"
  assert_log_absent "$term_logs" "inbound AO verification failed" "$DIRECTION_GOBGP_TO_GOBMP inbound AO verification failure"
  assert_log_absent "$init_logs"$'\n'"$term_logs" "tcp-ao test bypass enabled" "test bypass marker"

  local init_bytes_up
  local init_bytes_down
  local term_bytes_up
  local term_bytes_down
  init_bytes_up="$(extract_latest_counter "$init_logs" "bytes_up" "goBGP-side proxy")"
  init_bytes_down="$(extract_latest_counter "$init_logs" "bytes_down" "goBGP-side proxy")"
  term_bytes_up="$(extract_latest_counter "$term_logs" "bytes_up" "goBMP-side proxy")"
  term_bytes_down="$(extract_latest_counter "$term_logs" "bytes_down" "goBMP-side proxy")"

  (( init_bytes_up > 0 )) || fail "$DIRECTION_GOBGP_TO_GOBMP missing bytes on goBGP-side proxy (bytes_up=$init_bytes_up)"
  (( term_bytes_up > 0 )) || fail "$DIRECTION_GOBGP_TO_GOBMP missing bytes on goBMP-side proxy (bytes_up=$term_bytes_up)"
  ok "$DIRECTION_GOBGP_TO_GOBMP traffic observed (goBGP bytes_up=$init_bytes_up, goBMP bytes_up=$term_bytes_up)"

  if (( init_bytes_down > 0 && term_bytes_down > 0 )); then
    ok "$DIRECTION_GOBMP_TO_GOBGP traffic observed (goBMP bytes_down=$term_bytes_down, goBGP bytes_down=$init_bytes_down)"
  elif is_true "$REQUIRE_BIDIRECTIONAL_TRAFFIC"; then
    fail "$DIRECTION_GOBMP_TO_GOBGP not observed (goBGP bytes_down=$init_bytes_down, goBMP bytes_down=$term_bytes_down) with REQUIRE_BIDIRECTIONAL_TRAFFIC=1"
  else
    warn "$DIRECTION_GOBMP_TO_GOBGP not observed (goBGP bytes_down=$init_bytes_down, goBMP bytes_down=$term_bytes_down); expected for one-way BMP streams"
  fi

  ok "test-validation-tcpao-proxy passed"
}

main "$@"
