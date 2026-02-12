#!/usr/bin/env bash
set -euo pipefail

mkdir -p /workspaces/.clab
bash .devcontainer/scripts/start-docker.sh || true

echo "[devcontainer] tool check"
rustc --version
cargo --version
jq --version
tcpdump --version | head -n 1
containerlab version | head -n 1
docker --version
docker compose version || true

if ! docker info >/dev/null 2>&1; then
  echo "[devcontainer] warning: docker daemon is not ready yet; run: bash .devcontainer/scripts/start-docker.sh" >&2
else
  echo "[devcontainer] docker storage driver: $(docker info --format '{{.Driver}}')"
fi
