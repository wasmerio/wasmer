#!/usr/bin/env sh
# Install package dependencies on Alpine linux.
#
# This script is used by the CI!

apk update
apk add bash mold make curl cmake ninja clang21 zstd-static llvm21-dev clang21-static llvm21-static ncurses-static zlib-static tar libxml2-static
