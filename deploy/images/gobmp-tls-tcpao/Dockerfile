ARG BASE_IMAGE=ghcr.io/asadarafat/gobmp:v1.0.4-alpha-tls

FROM rust:1-bookworm AS tcpao-builder
WORKDIR /src
COPY Cargo.toml rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY cmd ./cmd
COPY src ./src
RUN cargo build --release

FROM ${BASE_IMAGE}

USER root
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates iproute2 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=tcpao-builder /src/target/release/tcpao-proxy /usr/local/bin/tcpao-proxy
COPY deploy/config/terminator.toml.tmpl /etc/tcpao-proxy/terminator.toml.tmpl
COPY deploy/entrypoint/terminator.sh /usr/local/bin/tcpao-terminator-entrypoint.sh
RUN chmod +x /usr/local/bin/tcpao-proxy /usr/local/bin/tcpao-terminator-entrypoint.sh

ENV LISTEN_AO=0.0.0.0:1790
ENV FORWARD_PLAIN=127.0.0.1:11019
ENV KEY_ID=1

ENTRYPOINT ["/usr/local/bin/tcpao-terminator-entrypoint.sh"]
