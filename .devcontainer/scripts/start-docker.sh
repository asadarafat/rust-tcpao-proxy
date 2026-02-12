#!/usr/bin/env bash
set -euo pipefail

DOCKER_STORAGE_DRIVER="${DOCKER_STORAGE_DRIVER:-vfs}"

docker_ready() {
  docker info >/dev/null 2>&1
}

current_driver() {
  docker info --format '{{.Driver}}' 2>/dev/null || true
}

start_dockerd() {
  nohup dockerd \
    --host=unix:///var/run/docker.sock \
    --storage-driver="${DOCKER_STORAGE_DRIVER}" \
    >/tmp/dockerd.log 2>&1 &
}

if docker_ready; then
  driver="$(current_driver)"
  if [[ "$driver" == "$DOCKER_STORAGE_DRIVER" ]]; then
    exit 0
  fi
  echo "[devcontainer] restarting dockerd with storage driver ${DOCKER_STORAGE_DRIVER} (was ${driver:-unknown})" >&2
  pkill -x dockerd || true
  rm -f /var/run/docker.pid || true
fi

if ! pgrep -x dockerd >/dev/null 2>&1; then
  start_dockerd
fi

for _ in $(seq 1 25); do
  if docker_ready; then
    driver="$(current_driver)"
    if [[ "$driver" == "$DOCKER_STORAGE_DRIVER" ]]; then
      exit 0
    fi
  fi
  sleep 1
done

echo "[devcontainer] warning: dockerd did not become ready with driver ${DOCKER_STORAGE_DRIVER} within 25 seconds" >&2
tail -n 120 /tmp/dockerd.log >&2 || true
exit 2
