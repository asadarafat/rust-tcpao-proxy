FROM rust:1-bookworm AS builder
WORKDIR /src
COPY Cargo.toml rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY cmd ./cmd
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates iproute2 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /src/target/release/tcpao-proxy /usr/local/bin/tcpao-proxy
COPY config/example.toml /etc/tcpao-proxy/config.toml
ENTRYPOINT ["/usr/local/bin/tcpao-proxy"]
CMD ["--mode", "initiator", "--config", "/etc/tcpao-proxy/config.toml"]
