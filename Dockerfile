FROM circleci/rust:1.39.0-stretch as wasmer-build-env
RUN sudo apt-get update && \
  sudo apt-get install -y --no-install-recommends \
  cmake \
  && sudo rm -rf /var/lib/apt/lists/*
RUN curl -SL https://releases.llvm.org/8.0.0/clang+llvm-8.0.0-x86_64-linux-gnu-ubuntu-16.04.tar.xz \
    | tar -xJC /home/circleci
ENV LLVM_SYS_80_PREFIX /home/circleci/clang+llvm-8.0.0-x86_64-linux-gnu-ubuntu-16.04/

FROM wasmer-build-env AS wasmer-debug-env
RUN sudo apt-get update && \
  sudo apt-get install -y --no-install-recommends \
  valgrind \
  && sudo rm -rf /var/lib/apt/lists/*

FROM wasmer-build-env AS wasmer-build
WORKDIR /home/circleci/wasmer
COPY . /home/circleci/wasmer
RUN sudo chmod -R 777 .
RUN cargo build --release --features backend-cranelift

FROM debian:stretch AS wasmer
WORKDIR /root/
COPY --from=wasmer-build /home/circleci/wasmer/target/release/wasmer .
ENTRYPOINT ["./wasmer"]
