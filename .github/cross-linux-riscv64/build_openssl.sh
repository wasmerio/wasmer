#!/bin/bash
set -x
set -euo pipefail

apt-get install wget

wget https://www.openssl.org/source/openssl-3.1.1.tar.gz

tar xf openssl-3.1.1.tar.gz
rm openssl-3.1.1.tar.gz
cd openssl-3.1.1

AR=riscv64-linux-gnu-ar NM=riscv64-linux-gnu-nm CC=riscv64-linux-gnu-gcc \
   ./Configure -static no-asm no-tests --prefix=/openssl_riscv64 linux64-riscv64

make -j
make install
