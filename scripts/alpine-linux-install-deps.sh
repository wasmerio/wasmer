#!/usr/bin/env sh
# Install package dependencies on Alpine linux.
#
# This script is used by the CI!

apk update
apk add build-base bash musl-dev mold clang curl tar make libtool libffi-dev gcc automake autoconf git openssl-dev g++ pkgconfig llvm18-dev clang18-static llvm18-static zlib-static
