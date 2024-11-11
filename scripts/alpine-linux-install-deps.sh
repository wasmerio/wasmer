#!/usr/bin/env sh
# Install package dependencies on Alpine linux.
#
# This script is used by the CI!

apk update
apk add bash mold make curl cmake ninja clang18 zstd-static llvm18-dev clang18-static llvm18-static ncurses-static zlib-static
ln -s /usr/bin/clang-18 /usr/bin/clang