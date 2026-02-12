#!/usr/bin/env bash
set -euo pipefail

mkdir -p /workspaces/.clab
bash .devcontainer/scripts/start-docker.sh

echo "[devcontainer] tool check"
docker --version
containerlab version | head -n 1
rustc --version
cargo --version
jq --version
tcpdump --version | head -n 1
