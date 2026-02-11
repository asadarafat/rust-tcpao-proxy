ARG BASE_IMAGE=ghcr.io/asadarafat/gobgp-stunnel:v3.37.0

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
COPY deploy/config/initiator.toml.tmpl /etc/tcpao-proxy/initiator.toml.tmpl
COPY deploy/entrypoint/initiator.sh /usr/local/bin/tcpao-initiator-entrypoint.sh
RUN chmod +x /usr/local/bin/tcpao-proxy /usr/local/bin/tcpao-initiator-entrypoint.sh

ENV LISTEN_PLAIN=127.0.0.1:5000
ENV KEY_ID=1

ENTRYPOINT ["/usr/local/bin/tcpao-initiator-entrypoint.sh"]
