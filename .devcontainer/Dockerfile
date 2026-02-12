FROM registry.fedoraproject.org/fedora:43

SHELL ["/bin/bash", "-lc"]

ARG CONTAINERLAB_VERSION=0.73.0
ARG DOCKER_COMPOSE_VERSION=2.29.2

ENV LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:${PATH}

RUN set -eux; \
    dnf -y update; \
    dnf -y install \
      bash-completion \
      ca-certificates \
      curl \
      dnf-plugins-core \
      findutils \
      git \
      iproute \
      iptables \
      iputils \
      jq \
      make \
      openssh-clients \
      procps-ng \
      shadow-utils \
      sudo \
      tar \
      tcpdump \
      unzip \
      wget \
      which \
      xz; \
    if ! dnf -y install moby-engine containerd runc; then \
      dnf config-manager --add-repo https://download.docker.com/linux/fedora/docker-ce.repo; \
      dnf -y install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin; \
    fi; \
    curl -fsSLo /tmp/containerlab.rpm "https://github.com/srl-labs/containerlab/releases/download/v${CONTAINERLAB_VERSION}/containerlab_${CONTAINERLAB_VERSION}_linux_amd64.rpm"; \
    dnf -y install /tmp/containerlab.rpm; \
    rm -f /tmp/containerlab.rpm; \
    if ! docker compose version >/dev/null 2>&1; then \
      mkdir -p /usr/local/lib/docker/cli-plugins; \
      curl -fsSL "https://github.com/docker/compose/releases/download/v${DOCKER_COMPOSE_VERSION}/docker-compose-linux-x86_64" \
        -o /usr/local/lib/docker/cli-plugins/docker-compose; \
      chmod +x /usr/local/lib/docker/cli-plugins/docker-compose; \
    fi; \
    docker compose version; \
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable; \
    rustup component add rustfmt clippy; \
    mkdir -p /workspaces/.clab; \
    dnf clean all; \
    rm -rf /var/cache/dnf
