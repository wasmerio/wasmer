#!/usr/bin/env sh
# Install package dependencies on Alpine linux.
#
# This script is used by the CI!

apk update
apk add bash mold make curl cmake ninja clang20 zstd-static llvm20-dev clang20-static llvm20-static ncurses-static zlib-static
