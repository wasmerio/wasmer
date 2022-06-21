FROM rust:1
#FROM ghcr.io/cross-rs/aarch64-unknown-linux-gnu:edge

# set CROSS_DOCKER_IN_DOCKER to inform `cross` that it is executed from within a container
ENV CROSS_DOCKER_IN_DOCKER=true

RUN cargo install cross
RUN dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -qy curl && \
    curl -sSL https://get.docker.com/ | sh