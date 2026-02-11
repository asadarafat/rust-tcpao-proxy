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
make doctor
cargo build
cargo run -- --mode initiator --config config/example.toml --dry-run
```

## Tooling

- `make fmt` for formatting
- `make lint` for clippy (`-D warnings`)
- `make test` for unit tests
- `make test-functional` for end-to-end traffic through two proxy instances (auto-skips if host lacks TCP-AO support/capability; may require `CAP_NET_ADMIN`/root)
- `Dockerfile` for containerized builds
- `scripts/doctor.sh` for host/kernel/tool preflight checks
