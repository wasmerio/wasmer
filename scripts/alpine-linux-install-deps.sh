#!/usr/bin/env sh
# Install package dependencies on Alpine linux.
#
# This script is used by the CI!

apk update
apk add bash make curl cmake ninja clang22 lld22 zstd-static llvm22-dev clang22-static llvm22-static ncurses-static zlib-static tar libxml2-static
ln -sf /usr/bin/llvm-config-22 /usr/bin/llvm-config

echo "Installed compiler/linker tools:"
for tool in cc clang clang-22 gcc ld ld.lld ld.lld-22 llvm-config llvm-config-22; do
    printf "  which %s: " "$tool"
    which "$tool" || true
done

if ! which cc >/dev/null 2>&1; then
    echo "cc was not found; checking for clang-22..."
    if which clang-22 >/dev/null 2>&1; then
        ln -sf "$(which clang-22)" /usr/bin/cc
        echo "Created /usr/bin/cc -> $(which clang-22)"
    else
        echo "error: neither cc nor clang-22 was found after installing Alpine dependencies." >&2
        exit 1
    fi
fi

if ! which ld >/dev/null 2>&1; then
    echo "ld was not found; checking for LLVM lld..."
    if which ld.lld-22 >/dev/null 2>&1; then
        ln -sf "$(which ld.lld-22)" /usr/bin/ld
        echo "Created /usr/bin/ld -> $(which ld.lld-22)"
    elif which ld.lld >/dev/null 2>&1; then
        ln -sf "$(which ld.lld)" /usr/bin/ld
        echo "Created /usr/bin/ld -> $(which ld.lld)"
    else
        echo "error: neither ld nor ld.lld was found after installing Alpine dependencies." >&2
        exit 1
    fi
fi

echo "Final compiler/linker tools:"
for tool in cc clang clang-22 gcc ld ld.lld ld.lld-22 llvm-config llvm-config-22; do
    printf "  which %s: " "$tool"
    which "$tool" || true
done
echo "cc version:"
cc --version
