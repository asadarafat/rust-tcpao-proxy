# tcpao-proxy (PoC scaffold)

Rust sidecar proxy scaffold for protecting the wire leg of BMP sessions with TCP-AO.

## Current state

- Project layout and modules are in place (`cmd/tcpao-proxy/main.rs`, `src/*`)
- CLI/config parsing, mode dispatch, and bidirectional forwarding skeleton are implemented
- Linux TCP keepalive tuning and fail-closed runtime behavior are wired
- Linux TCP-AO socket integration is implemented for:
  - outbound key install before `connect()`
  - listener policy install + AO required mode
  - inbound AO state verification

## Quick start

```bash
make tools
rustc --version
cargo --version
make doctor
make test
make test-functional
```

## BMP TCP-AO Use Case

This project implements a practical deployment model aligned with `draft-ietf-grow-bmp-tcp-ao-03`: protect the BMP transport leg with TCP-AO while keeping BMP applications operationally simple.

In this lab, goBGP acts as the BMP producer and goBMP as the collector. Adding Linux TCP-AO socket policy handling directly inside those Go applications would require invasive kernel/socket control paths in software that is primarily focused on routing and telemetry logic.

To avoid that, TCP-AO responsibility is isolated in a dedicated Rust sidecar proxy. The sidecar handles AO policy installation, AO-required listener behavior, and inbound AO verification through native Linux socket integration. goBGP and goBMP continue to use plain local TCP to the sidecar.

The result is a clean separation of concerns: AO is enforced on the wire path, and application behavior remains stable.

### Deployment Pattern: Docker "Baked Sidecar" (Single Container)

The deployment pattern is intentionally straightforward:

- Image A: `goBGP + tcpao-proxy (initiator)`
- Image B: `goBMP + tcpao-proxy (terminator)`

Inside the pair, traffic flows like this:

1. goBGP sends BMP to local initiator proxy (`127.0.0.1:5000`).
2. Initiator proxy opens wire connection to terminator proxy (`10.10.10.2:1790`) with TCP-AO.
3. Terminator proxy verifies AO and forwards plain TCP locally to goBMP backend (`127.0.0.1:11019`).

This keeps AO enforcement on the inter-node path while preserving application-level simplicity inside each container.

## Route Validation Workflow

For iterative testing, deployment and validation are separated.

Deploy only:

```bash
make test-validation-tcpao-proxy-bgp-route-deploy
```

This runs:

```make
containerlab deploy -t deploy/containerlab/tcpao-bmp.clab.yml --reconfigure
```

This redeploys the lab to a known-good state and exits.

Validate only (assumes the lab is already running):

```bash
make test-validation-tcpao-proxy-bgp-route-validate-only
```

This runs:

```make
MAX_WAIT_SECS=$${MAX_WAIT_SECS:-30} JQ_INSTALL_TIMEOUT_SECS=$${JQ_INSTALL_TIMEOUT_SECS:-20} DEPLOY_LAB=0 ./scripts/test-validation-tcpao-proxy-bgp-route.sh
```

With `DEPLOY_LAB=0`, the script skips redeployment and performs validation directly:

1. Confirms clab containers are running and listeners are up.
2. Starts/ensures goBMP backend dump path.
3. Starts goBGP with BMP export pointed at the local initiator proxy.
4. Injects route (`203.0.113.0/24` by default) into goBGP.
5. Verifies the route appears in goBMP dump output.
6. Verifies AO policy logs and forwarded-byte evidence on both proxies.

For a single-command flow, `make test-validation-tcpao-proxy-bgp-route` still performs deploy + validate in one run.

Containerlab demo:
https://github.com/user-attachments/assets/ca14fe7a-12df-4914-8e36-98ea88d9101a

## Tooling

- `make fmt` for formatting
- `make lint` for clippy (`-D warnings`)
- `make test` for unit tests
- `make test-functional` for end-to-end traffic through two proxy instances (always clears `TCPAO_PROXY_TEST_NO_AO` first, uses real TCP-AO when available, and falls back to debug/test-only no-AO mode when unavailable; may require `CAP_NET_ADMIN`/root)
- `make test-functional-strict` for end-to-end traffic through two proxy instances with real TCP-AO required (`TCPAO_PROXY_TEST_REQUIRE_AO=1`, no fallback)
- `make test-validation-tcpao-proxy` to redeploy `deploy/containerlab/tcpao-bmp.clab.yml` with `--reconfigure`, inject traffic, and validate AO + forwarding evidence from container logs (requires containerlab/docker privileges, use `sudo -E` if needed)
  - Set `REQUIRE_BIDIRECTIONAL_TRAFFIC=1` for strict reverse-direction validation; with this enabled, backend mode defaults to `BACKEND_MODE=echo` so both `from-goBGP-to-goBMP` and `from-goBMP-to-goBGP` data paths are validated.
  - Validation output now also prints effective runtime config context for goBGP/goBMP and tcpao-proxy (config files + process command lines inside both containers).
  - During execution, it prints a `traffic injection plan` section describing exactly how payload is injected and what path/direction is being validated.
- `make test-validation-tcpao-proxy-bgp-route` to redeploy the lab, start goBGP BMP export + goBMP file dump, inject a route in goBGP, and verify the prefix is received by goBMP over the AO-protected path.
  - For modular workflow, use `make test-validation-tcpao-proxy-bgp-route-deploy` once, then run `make test-validation-tcpao-proxy-bgp-route-validate-only` repeatedly without redeploying.
  - Route evidence is pretty-printed with `jq`; if `jq` is missing the script attempts auto-install and falls back to non-pretty evidence output if install is not possible.
  - This make target embeds `MAX_WAIT_SECS=30` and `JQ_INSTALL_TIMEOUT_SECS=20` by default (you can still override by exporting either variable).
- `make tools` for Rust tooling bootstrap via Fedora `dnf` (uses `~/proxy` + `sudo_dnf` if available)
- `Dockerfile` for containerized builds
- `scripts/doctor.sh` for host/kernel/tool preflight checks
- `docs/deployment-runbook.md` and `deploy/` for combined sidecar image deployment patterns
