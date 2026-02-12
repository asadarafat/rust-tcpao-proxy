#!/usr/bin/env bash
set -euo pipefail

if docker info >/dev/null 2>&1; then
  exit 0
fi

if ! pgrep -x dockerd >/dev/null 2>&1; then
  nohup dockerd --host=unix:///var/run/docker.sock >/tmp/dockerd.log 2>&1 &
fi

for _ in $(seq 1 25); do
  if docker info >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done

echo "[devcontainer] warning: dockerd did not become ready within 25 seconds" >&2
tail -n 120 /tmp/dockerd.log >&2 || true
exit 2
