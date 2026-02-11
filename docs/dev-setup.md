# Development setup

## 1) Host prerequisites

- Linux host kernel with TCP-AO support (`CONFIG_TCP_AO=y`)
- Rust toolchain (`rustup`, `cargo`, `clippy`, `rustfmt`)
- Network debug tooling (`ss`, `tcpdump`)

## 2) Bootstrap

```bash
make doctor
cargo build
```

## 3) Validate config only

```bash
cargo run -- --mode initiator --config config/example.toml --dry-run
```

## 4) AO operational notes

- TCP-AO must be configured on sockets before connect/accept path finalization.
- TCP-AO provides integrity/authentication, not encryption.
- Avoid logging key material.
- Functional AO tests may need elevated privileges (`CAP_NET_ADMIN` or root).
- Functional test fallback: in debug/test runs only, `TCPAO_PROXY_TEST_NO_AO=1` bypasses AO setsockopt so end-to-end byte forwarding can still be validated on hosts without TCP-AO support.
