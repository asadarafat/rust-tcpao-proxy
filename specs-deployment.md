# Deployment Implementation Specification

## 1. Purpose

Define concrete deployment implementation for these two combined images (one container per side):

- `gobmp-tls + tcpao-sidecar` (terminator side)
- `gobgp-stunnel + tcpao-sidecar` (initiator side)

This document expands `specs.md` section `12. Deployment patterns` into executable artifacts, runtime contracts, and CI/CD flow.

## 2. Scope and boundaries

In scope:

- Build and publish two container images that each run, in the same container:
  - the existing app process (`gobmp-tls` or `gobgp-stunnel`)
  - `tcpao-proxy` sidecar process
- Wire-leg protection using TCP-AO between sidecars.
- Runtime config and secret injection model.
- Host/kernel checks and verification commands.

Out of scope:

- App protocol changes inside gobmp/gobgp.
- Automatic TCP-AO key rotation orchestration.
- Encryption claims for TCP-AO wire leg (TCP-AO is auth/integrity, not privacy).

## 3. Target topology

BMP session is split into three legs:

1. Local app leg (inside source container): `gobgp-stunnel -> tcpao-proxy initiator` (plain TCP on localhost)
2. Wire leg (between the two combined containers/nodes): `tcpao-proxy initiator <-> tcpao-proxy terminator` (TCP-AO)
3. Local app leg (inside destination container): `tcpao-proxy terminator -> gobmp-tls` (plain TCP on localhost)

Reference ports (override as needed):

- `GOBGP_LOCAL_BMP_OUT=127.0.0.1:5000`
- `TCPAO_WIRE_LISTEN=0.0.0.0:1790` (terminator)
- `TCPAO_WIRE_REMOTE=<terminator-ip>:1790` (initiator)
- `GOBMP_LOCAL_BMP_IN=127.0.0.1:11019`

## 4. Host prerequisites

Deployment host must satisfy:

- Linux kernel with `CONFIG_TCP_AO=y`
- Runtime permission for AO socket operations (typically root or `CAP_NET_ADMIN`)
- Container engine networking that preserves direct TCP endpoint identity for AO peer policy

Validation commands:

```bash
grep -E '^CONFIG_TCP_AO=' /boot/config-$(uname -r)
python3 - <<'PY'
import socket, errno
TCP_AO_INFO = 40
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
try:
    s.getsockopt(socket.IPPROTO_TCP, TCP_AO_INFO, 64)
    print("TCP-AO runtime available")
except OSError as e:
    print("TCP-AO runtime unavailable:", e)
PY
```

## 5. Artifact model

Publish two images:

- `ghcr.io/<org>/gobmp-tls-tcpao:<tag>`
- `ghcr.io/<org>/gobgp-stunnel-tcpao:<tag>`

Each image contains:

- app binaries and runtime dependencies from existing gobmp build pattern
- `/usr/local/bin/tcpao-proxy`
- startup script that launches app + sidecar and exits non-zero if either process fails
- config template under `/etc/tcpao-proxy/config.toml`

## 6. Build strategy

Use multi-stage Docker builds:

1. Build `tcpao-proxy` in this repo with `cargo build --release`
2. Build/obtain app binaries (`gobmp` or `gobgp` + `stunnel`) using existing `_gobmp` workflows as base pattern
3. Final runtime image copies both app and sidecar binaries

Recommended runtime base:

- `debian:bookworm-slim` or `ubuntu:24.04` with:
  - `ca-certificates`
  - `iproute2`
  - `stunnel` (for `gobgp-stunnel-tcpao` image)

## 7. Runtime startup contract

Use one entrypoint script per role.

Initiator entrypoint responsibilities:

1. Validate required env vars (`TCPAO_KEY`, remote endpoint, local app endpoint)
2. Render `/etc/tcpao-proxy/config.toml` from template
3. Start app process (`gobgp-stunnel` stack)
4. Start `tcpao-proxy --mode initiator --config /etc/tcpao-proxy/config.toml`
5. `wait -n` and terminate peer process on failure

Terminator entrypoint responsibilities:

1. Validate required env vars
2. Render config with `listen_ao` and `forward_plain`
3. Start app process (`gobmp-tls`)
4. Start `tcpao-proxy --mode terminator --config /etc/tcpao-proxy/config.toml`
5. `wait -n` and terminate peer process on failure

## 8. TCP-AO config templates

Initiator template (`/etc/tcpao-proxy/initiator.toml.tmpl`):

```toml
[global]
log_format = "json"
idle_timeout_secs = 120
tcp_keepalive = true
keepalive_time_secs = 30
keepalive_intvl_secs = 10
keepalive_probes = 3

[initiator]
listen_plain = "${LISTEN_PLAIN}"
remote_ao = "${REMOTE_AO}"

[[ao_policy]]
name = "bmp-wire"
peer_ip = "${PEER_IP}"
peer_port = ${PEER_PORT}
keyid = ${KEY_ID}
mac_alg = "hmac-sha256"
key_source = "env:TCPAO_KEY"
```

Terminator template (`/etc/tcpao-proxy/terminator.toml.tmpl`):

```toml
[global]
log_format = "json"
idle_timeout_secs = 120
tcp_keepalive = true
keepalive_time_secs = 30
keepalive_intvl_secs = 10
keepalive_probes = 3

[terminator]
listen_ao = "${LISTEN_AO}"
forward_plain = "${FORWARD_PLAIN}"

[[ao_policy]]
name = "bmp-wire"
peer_ip = "${PEER_IP}"
peer_port = ${PEER_PORT}
keyid = ${KEY_ID}
mac_alg = "hmac-sha256"
key_source = "env:TCPAO_KEY"
```

## 9. Secret and key handling

Required env vars:

- `TCPAO_KEY` (shared secret)
- `KEY_ID` (default `1`)

Rules:

- Never bake keys into image layers.
- Never log key bytes.
- Prefer orchestrator secret mount + env injection at runtime.

## 10. CI/CD implementation

Add two GitHub workflows in this repo (or in the image-owning repo):

- `.github/workflows/publish_gobmp_tls_tcpao.yml`
- `.github/workflows/publish_gobgp_stunnel_tcpao.yml`

Each workflow should:

1. Checkout repo(s)
2. Build `tcpao-proxy` release binary
3. Build role-specific image
4. Login to GHCR
5. Push tags:
   - `${{ github.event.inputs.image_tag }}`
   - `latest`
6. Emit image digest in job summary

Source references for existing image publishing pattern:

- `_gobmp/.github/workflows/ghcr_publisher_bmp_tls.yml`

## 11. Deployment patterns

### 11.1 Single-host Docker lab

Use user-defined bridge network and fixed container names/IPs.

- Start `gobmp-tls-tcpao` first (terminator)
- Start `gobgp-stunnel-tcpao` with `REMOTE_AO=<terminator-ip>:1790`
- Ensure both containers run with capability required for AO operations if needed:
  - `--cap-add NET_ADMIN`

### 11.2 Multi-node / containerlab

Map one combined image to each node and place AO wire leg on the inter-node link.

Minimum checks:

- terminator logs: listener AO policies installed
- initiator logs: outbound AO policy applied before connect
- traffic forwarding works end-to-end

## 12. Validation and acceptance

Functional checks:

1. Positive path:
   - traffic passes from source app to destination app
2. Negative key mismatch:
   - wrong key blocks session (fail closed)
3. Restart behavior:
   - sidecar failure tears down corresponding local leg

Commands:

```bash
# host preflight
./scripts/doctor.sh

# repo tests
make test
make test-functional-strict

# runtime inspection
ss -ltnp | grep -E '1790|5000|11019'
```

Pass criteria:

- `make test-functional-strict` succeeds on AO-capable host
- logs show AO policy install/apply and no key leakage
- wrong-key scenario fails closed

## 13. Implemented artifacts

The following files are implemented in this repository:

- `deploy/images/gobmp-tls-tcpao/Dockerfile`
- `deploy/images/gobgp-stunnel-tcpao/Dockerfile`
- `deploy/entrypoint/initiator.sh`
- `deploy/entrypoint/terminator.sh`
- `deploy/config/initiator.toml.tmpl`
- `deploy/config/terminator.toml.tmpl`
- `deploy/containerlab/tcpao-bmp.clab.yml`
- `docs/deployment-runbook.md`
- `.github/workflows/publish_gobmp_tls_tcpao.yml`
- `.github/workflows/publish_gobgp_stunnel_tcpao.yml`

## 14. Next hardening tasks

1. Pin known-good app startup commands per base image (`APP_CMD`) to avoid manual override.
2. Add containerized smoke test that boots both combined images and validates end-to-end traffic.
3. Add a negative-path CI scenario (wrong key) to assert fail-closed behavior automatically.
