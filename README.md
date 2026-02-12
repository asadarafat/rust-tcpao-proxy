# tcpao-proxy (PoC scaffold)

Rust sidecar proxy scaffold for protecting the wire leg of BMP sessions with TCP-AO.

## Why This Repo Exists

This project implements a practical deployment model aligned with `draft-ietf-grow-bmp-tcp-ao-03`: protect the BMP transport leg with TCP-AO while keeping BMP applications operationally simple.

In this lab, goBGP is the BMP producer and goBMP is the collector. Instead of embedding low-level Linux TCP-AO socket policy logic directly in those Go applications, AO responsibilities are isolated in a Rust sidecar proxy that uses native Linux socket hooks.

The result is clean separation of concerns: AO on the wire path, minimal disruption to application logic.

## Try It First (Fast Path)

If you are new to this repo, start with this path.

1. Open in GitHub Codespaces (this repo includes a Fedora 43 devcontainer).
2. Wait for container startup to complete.
3. Run:

```bash
make test-validation-tcpao-proxy-bgp-route-deploy
make test-validation-tcpao-proxy-bgp-route-validate-only
```

If successful, the second command ends with:

```text
[ok] test-validation-tcpao-proxy-bgp-route passed
```

Demo:
https://github.com/user-attachments/assets/ca14fe7a-12df-4914-8e36-98ea88d9101a

## What You Just Validated

The validation confirms this end-to-end behavior:

1. goBGP exports BMP toward local tcpao-proxy.
2. tcpao-proxy initiator applies outbound TCP-AO on the wire leg.
3. tcpao-proxy terminator enforces listener AO policy.
4. goBMP receives BMP route updates.
5. Logs and byte counters show traffic actually crossed the AO-protected path.

## Deployment Pattern

Docker baked sidecar pattern (single container image per role):

- Image A: `goBGP + tcpao-proxy (initiator)`
- Image B: `goBMP + tcpao-proxy (terminator)`

Traffic path:

1. goBGP -> initiator plain listener (`127.0.0.1:5000`)
2. initiator -> terminator wire leg with TCP-AO (`10.10.10.2:1790`)
3. terminator -> goBMP backend plain TCP (`127.0.0.1:11019`)

## Validation Commands

Deploy lab only:

```bash
make test-validation-tcpao-proxy-bgp-route-deploy
```

Validate against already-deployed lab:

```bash
make test-validation-tcpao-proxy-bgp-route-validate-only
```

One-shot deploy + validate:

```bash
make test-validation-tcpao-proxy-bgp-route
```

## Codespaces Environment (Fedora 43)

The devcontainer includes:

- Docker daemon (started automatically)
- containerlab
- jq, curl, wget, tcpdump
- Rust toolchain (`rustc`, `cargo`, `rustfmt`, `clippy`)
- Make and Linux networking utilities used by scripts

Important: TCP-AO behavior still depends on host kernel support.

## Development Status (PoC)

- Project layout and modules are in place (`cmd/tcpao-proxy/main.rs`, `src/*`)
- CLI/config parsing, mode dispatch, and forwarding skeleton are implemented
- Linux keepalive tuning and fail-closed behavior are wired
- Linux TCP-AO integration is implemented for:
  - outbound key install before `connect()`
  - listener policy install with AO-required mode
  - inbound AO state verification

## Additional Commands

- `make fmt` for formatting
- `make lint` for clippy (`-D warnings`)
- `make test` for unit tests
- `make test-functional` for end-to-end traffic through two proxy instances
- `make test-functional-strict` for real TCP-AO required mode
- `make test-validation-tcpao-proxy` for payload-injection validation on containerlab topology
- `make tools` for Rust tooling bootstrap via Fedora `dnf`

More operational details:

- `docs/deployment-runbook.md`
- `deploy/`
- `scripts/doctor.sh`
