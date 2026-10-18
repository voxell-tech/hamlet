# syntax=docker/dockerfile:1
#
# One image for both roles of hamlet_workspace: the manager and the
# in-actor worker. Build it from the repository root, because Cargo
# needs the whole workspace and its Cargo.lock to resolve this crate:
#
#   docker build -f crates/hamlet_workspace/Dockerfile \
#     -t hamlet-workspace:local .
#
# The image carries no configuration and no keys. Mount a configuration
# file and pass its path: `worker /etc/hamlet-workspace/worker.json`.
# See README.md next to this file.

# Rust 1.99 and Debian 12, pinned by index digest. The runtime is the
# same Debian release, so the binary's glibc matches.
ARG RUST_IMAGE=rust:1.99-bookworm@sha256:e12566d5abc17a0da741628782cdb504b68b8961f5bfcf677936bf3e8b32de29
ARG RUNTIME_IMAGE=debian:bookworm-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587

FROM ${RUST_IMAGE} AS build
WORKDIR /src
# Every workspace member and path dependency must be present for Cargo
# to load the workspace, but only this crate and its dependencies are
# compiled. `--locked` refuses to build if Cargo.lock would change.
COPY Cargo.toml Cargo.lock ./
COPY src src
COPY examples examples
COPY crates crates
COPY vendor vendor
RUN cargo build --locked --release -p hamlet_workspace \
        --bin hamlet-workspace \
    && install -D -m 0755 target/release/hamlet-workspace \
        /out/hamlet-workspace

FROM ${RUNTIME_IMAGE}
# bubblewrap is pinned to the Debian 12 build used by the local runsc
# isolation probes; the build fails rather than silently changing it.
# The worker runs commands with /bin/bash -c, and only /usr, /bin,
# /lib and /lib64 are visible inside a command, so the ordinary tools
# agents use must be installed here. ca-certificates is for the
# manager's HTTPS and gRPC clients.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bubblewrap=0.8.0-2+deb12u1 \
        bash \
        ca-certificates \
        coreutils \
        curl \
        diffutils \
        file \
        findutils \
        gawk \
        git \
        grep \
        gzip \
        less \
        patch \
        procps \
        python3 \
        ripgrep \
        sed \
        socat \
        tar \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/hamlet-workspace /usr/local/bin/hamlet-workspace
# No default role: choose `manager CONFIG` or `worker CONFIG`
# explicitly. Without arguments the binary prints its usage.
ENTRYPOINT ["/usr/local/bin/hamlet-workspace"]
