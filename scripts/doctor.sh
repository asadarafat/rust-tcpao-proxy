#!/usr/bin/env bash
set -euo pipefail

missing=0

check_cmd() {
  local cmd="$1"
  if command -v "$cmd" >/dev/null 2>&1; then
    echo "[ok] found $cmd"
  else
    echo "[missing] $cmd"
    missing=1
  fi
}

check_cmd rustc
check_cmd cargo
check_cmd ss

if command -v tcpdump >/dev/null 2>&1; then
  echo "[ok] found tcpdump"
else
  echo "[warn] tcpdump not found (optional but recommended for packet validation)"
fi

if [[ -r /usr/include/linux/tcp.h ]] && grep -q "TCP_AO" /usr/include/linux/tcp.h; then
  echo "[ok] tcp_ao constants present in linux headers"
else
  echo "[warn] tcp_ao constants not found in /usr/include/linux/tcp.h"
fi

if [[ -r /proc/config.gz ]] && zgrep -q "CONFIG_TCP_AO=y" /proc/config.gz; then
  echo "[ok] kernel reports CONFIG_TCP_AO=y"
elif [[ -r /boot/config-$(uname -r) ]] && grep -q "CONFIG_TCP_AO=y" "/boot/config-$(uname -r)"; then
  echo "[ok] kernel reports CONFIG_TCP_AO=y"
else
  echo "[warn] unable to confirm CONFIG_TCP_AO=y; verify host kernel manually"
fi

if [[ "$missing" -ne 0 ]]; then
  echo "[fail] install missing tools listed above"
  exit 1
fi

echo "[ok] baseline environment checks complete"
