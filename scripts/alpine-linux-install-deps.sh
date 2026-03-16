#!/usr/bin/env sh
# Install package dependencies on Alpine linux.
#
# This script is used by the CI!

apk update
apk add bash make curl cmake ninja clang22 zstd-static llvm22-dev clang22-static llvm22-static ncurses-static zlib-static tar libxml2-static
ln -s /usr/bin/llvm-config-22 /usr/bin/llvm-config
