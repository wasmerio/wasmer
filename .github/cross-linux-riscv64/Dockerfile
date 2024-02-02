FROM debian:stable AS openssl_riscv64
#FROM ghcr.io/cross-rs/riscv64gc-unknown-linux-gnu:edge AS openssl_riscv64

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

#COPY install_deb.sh /

#install libssl-dev for riscv64!
#RUN /install_deb.sh riscv64 libssl-dev
#RUN dpkg --add-architecture riscv64
#RUN apt-get update
#RUN apt-get install libssl-dev:riscv64
#ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/usr/include
#ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/usr/lib/riscv64-linux-gnu


# install rust tools
RUN curl --proto "=https" --tlsv1.2 --retry 3 -sSfL https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup -v toolchain install 1.73
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
    libssl-dev \
    pkg-config \
    libc6-dev-riscv64-cross

ENV CROSS_TOOLCHAIN_PREFIX=riscv64-linux-gnu-
ENV CROSS_SYSROOT=/usr/riscv64-linux-gnu
ENV CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER="$CROSS_TOOLCHAIN_PREFIX"gcc \
    AR_riscv64gc_unknown_linux_gnu="$CROSS_TOOLCHAIN_PREFIX"ar \
    CC_riscv64gc_unknown_linux_gnu="$CROSS_TOOLCHAIN_PREFIX"gcc \
    CXX_riscv64gc_unknown_linux_gnu="$CROSS_TOOLCHAIN_PREFIX"g++ \
    CFLAGS_riscv64gc_unknown_linux_gnu="-march=rv64gc -mabi=lp64d" \
    BINDGEN_EXTRA_CLANG_ARGS_riscv64gc_unknown_linux_gnu="--sysroot=$CROSS_SYSROOT" \
    QEMU_LD_PREFIX="$CROSS_SYSROOT" \
    RUST_TEST_THREADS=1 \
    PKG_CONFIG_PATH="/usr/lib/riscv64-linux-gnu/pkgconfig/:${PKG_CONFIG_PATH}"

RUN rustup target add riscv64gc-unknown-linux-gnu --toolchain 1.73-x86_64-unknown-linux-gnu

#compile libssl-dev for riscv64!
COPY build_openssl.sh /
RUN /build_openssl.sh
ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_INCLUDE_DIR=/openssl_riscv64/include
ENV RISCV64GC_UNKNOWN_LINUX_GNU_OPENSSL_LIB_DIR=/openssl_riscv64/lib
