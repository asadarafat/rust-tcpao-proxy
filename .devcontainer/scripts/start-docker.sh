#!/usr/bin/env bash
set -euo pipefail

if docker info >/dev/null 2>&1; then
  exit 0
fi

if ! pgrep -x dockerd >/dev/null 2>&1; then
  nohup dockerd --host=unix:///var/run/docker.sock >/tmp/dockerd.log 2>&1 &
fi

for _ in $(seq 1 60); do
  if docker info >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done

echo "[devcontainer] dockerd failed to start within 60 seconds" >&2
tail -n 200 /tmp/dockerd.log >&2 || true
exit 1
