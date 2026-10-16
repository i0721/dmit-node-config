ARG BASE=debian:12
FROM ${BASE}
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends systemd systemd-sysv ca-certificates curl gnupg git build-essential pkg-config libssl-dev python3 unzip iproute2 util-linux xz-utils cron procps dbus && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup.sh && sh /tmp/rustup.sh -y --profile minimal --default-toolchain 1.99.1 && rm /tmp/rustup.sh
ENV PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
WORKDIR /work
COPY . /work
RUN bash tests/integration/install-runtime.sh
RUN cargo install cargo-cyclonedx --version 0.5.9 --locked && cargo build --locked --features integration-test --target-dir /integration-target
ENTRYPOINT ["/bin/bash", "/work/tests/integration/boot.sh"]
CMD ["/sbin/init"]
