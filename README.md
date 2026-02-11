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

## Tooling

- `make fmt` for formatting
- `make lint` for clippy (`-D warnings`)
- `make test` for unit tests
- `make test-functional` for end-to-end traffic through two proxy instances (always clears `TCPAO_PROXY_TEST_NO_AO` first, uses real TCP-AO when available, and falls back to debug/test-only no-AO mode when unavailable; may require `CAP_NET_ADMIN`/root)
- `make test-functional-strict` for end-to-end traffic through two proxy instances with real TCP-AO required (`TCPAO_PROXY_TEST_REQUIRE_AO=1`, no fallback)
- `make tools` for Rust tooling bootstrap via Fedora `dnf` (uses `~/proxy` + `sudo_dnf` if available)
- `Dockerfile` for containerized builds
- `scripts/doctor.sh` for host/kernel/tool preflight checks
