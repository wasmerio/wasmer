FROM ubuntu:19.04

ARG RUST_TOOLCHAIN="nightly"

ENV CARGO_HOME=/usr/local/rust
ENV RUSTUP_HOME=/usr/local/rust
ENV PATH="$PATH:$CARGO_HOME/bin"

RUN apt-get update \
    && apt-get -y install sudo strace curl cmake pkg-config python libssl-dev llvm-dev libz-dev gnuplot-nox \
    && echo '%sudo ALL=(ALL) NOPASSWD:ALL' >> /etc/sudoers \
    && echo '%wheel ALL=(ALL) NOPASSWD:ALL' >> /etc/sudoers \
    && curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain "$RUST_TOOLCHAIN"
