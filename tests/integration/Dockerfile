ARG BASE=debian:12
FROM ${BASE}
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && sed -i 's|http://|https://|g' /etc/apt/sources.list.d/*.sources
RUN apt-get update && apt-get install -y --no-install-recommends systemd systemd-sysv ca-certificates curl gnupg git build-essential pkg-config libssl-dev unzip iproute2 util-linux xz-utils cron procps dbus dnsutils dnsmasq-base && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup.sh && sh /tmp/rustup.sh -y --profile minimal --default-toolchain 1.99.1 && rm /tmp/rustup.sh
ENV PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
WORKDIR /work
RUN cargo install cargo-cyclonedx --version 0.5.9 --locked
COPY . /work
RUN cargo build --locked --features integration-test --target-dir /integration-target
RUN bash tests/integration/install-runtime.sh
ENTRYPOINT ["/bin/bash", "/work/tests/integration/boot.sh"]
CMD ["/sbin/init"]
