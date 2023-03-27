FROM debian:sid
#FROM ghcr.io/cross-rs/aarch64-unknown-linux-gnu:edge

# set CROSS_DOCKER_IN_DOCKER to inform `cross` that it is executed from within a container
ENV CROSS_DOCKER_IN_DOCKER=true

RUN apt-get update  && \
    apt-get install --assume-yes --no-install-recommends \
        ca-certificates \
        curl \
        cpio \
        sharutils \
        gnupg \
        build-essential \
        libc6-dev

RUN curl --proto "=https" --tlsv1.2 --retry 3 -sSfL https://sh.rustup.rs | sh -s -- -y

ENV PATH="/root/.cargo/bin:${PATH}"

RUN cargo install cross

COPY install_deb.sh /

#install libssl-dev for riscv64!
RUN /install_deb.sh riscv64 libssl-dev
ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include/openssl
ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/riscv64-linux-gnu

# add docker the manual way
COPY install_docker.sh /
RUN /install_docker.sh

RUN apt-get update && \
    apt-get install --assume-yes --no-install-recommends \
    docker-ce \
    docker-ce-cli \
    containerd.io \
    docker-buildx-plugin \
    docker-compose-plugin

RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc-riscv64-linux-gnu \
    g++-riscv64-linux-gnu \
    qemu-user-static \
    pkg-config \
    libc6-dev-riscv64-cross

ENV CROSS_TOOLCHAIN_PREFIX=riscv64-linux-gnu-
ENV CROSS_SYSROOT=/usr/riscv64-linux-gnu
ENV AR_riscv64gc_unknown_linux_gnu="$CROSS_TOOLCHAIN_PREFIX"ar \
    CC_riscv64gc_unknown_linux_gnu="$CROSS_TOOLCHAIN_PREFIX"gcc \
    CXX_riscv64gc_unknown_linux_gnu="$CROSS_TOOLCHAIN_PREFIX"g++ \
    BINDGEN_EXTRA_CLANG_ARGS_riscv64gc_unknown_linux_gnu="--sysroot=$CROSS_SYSROOT" \
    QEMU_LD_PREFIX="$CROSS_SYSROOT" \
    RUST_TEST_THREADS=1 \
    PKG_CONFIG_PATH="/usr/lib/riscv64-linux-gnu/pkgconfig/:${PKG_CONFIG_PATH}"

ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/riscv64-linux-gnu
ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include/openssl