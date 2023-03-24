#FROM rust:1
FROM ghcr.io/cross-rs/riscv64gc-unknown-linux-gnu:latest

# Prepare script to grab Debian port package
#COPY install_deb.sh /

# set CROSS_DOCKER_IN_DOCKER to inform `cross` that it is executed from within a container
ENV CROSS_DOCKER_IN_DOCKER=true

#RUN cargo install cross
RUN apt-get update && \
    apt-get install -qy curl gcc-riscv64-linux-gnu libssl-dev:riscv64 && \
    curl -sSL https://get.docker.com/ | sh
# Needed for openssl-sys crate
#RUN /install_deb.sh riscv64 libssl-dev

#ENV OPENSSL_VERSION="3.1.0"

#RUN set -x \
#    ### BUILD OpenSSL
#    && wget --no-check-certificate -O /tmp/openssl-${OPENSSL_VERSION}.tar.gz "https://www.openssl.org/source/openssl-${OPENSSL_VERSION}.tar.gz" \
#    && tar -xvf /tmp/openssl-${OPENSSL_VERSION}.tar.gz -C /tmp/ \
#    && rm -rf /tmp/openssl-${OPENSSL_VERSION}.tar.gz \ 
#    && cd /tmp/openssl-${OPENSSL_VERSION} \
#    && ./Configure linux64-riscv64 --cross-compile-prefix=riscv64-linux-gnu- --prefix=/opt/riscv64 shared\
#    && make \
#    && make install \
#    && cd .. \
#    && rm -rf openssl-${OPENSSL_VERSION}