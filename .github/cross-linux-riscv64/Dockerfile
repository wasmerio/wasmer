FROM rust:1
#FROM ghcr.io/cross-rs/riscv64gc-unknown-linux-gnu:edge

# Prepare script to grab Debian port package
#COPY install_deb.sh /

# set CROSS_DOCKER_IN_DOCKER to inform `cross` that it is executed from within a container
ENV CROSS_DOCKER_IN_DOCKER=true

RUN cargo install cross
RUN apt-get update && \
    apt-get install -qy curl && \
    curl -sSL https://get.docker.com/ | sh
# Needed for openssl-sys crate
#RUN /install_deb.sh riscv64 libssl-dev