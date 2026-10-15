#!/bin/sh
# Rebuilds the prebuilt libc2 test binaries in this directory.
#
#   build.sh <sysroot-wasixv2> <libc2-checkout>
#
# environment: CC (default clang, must be the wasix-org/llvm-project clang
#              with the wasm32-wasixv2 driver), WASM_TOOLS (default wasm-tools)
set -eu

[ $# -eq 2 ] || { echo "usage: $0 <sysroot> <libc2-checkout>" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
SYSROOT=$(cd "$1" && pwd)
LIBC2=$(cd "$2" && pwd)
CC=${CC:-clang}
WASM_TOOLS=${WASM_TOOLS:-wasm-tools}
BUILTINS=$SYSROOT/lib/wasm32-wasixv2/libclang_rt.builtins.a

build() {
	"$CC" --target=wasm32-wasixv2 --no-default-config --sysroot="$SYSROOT" \
		-Os -nodefaultlibs "$1" -lpthread -lc "$BUILTINS" -o "$HERE/$2"
	"$WASM_TOOLS" strip --all "$HERE/$2" -o "$HERE/$2"
	"$WASM_TOOLS" validate "$HERE/$2"
	chmod -x "$HERE/$2"
}

build "$HERE/hello.c" hello.wasm
build "$LIBC2/wasix2/test/smoke.c" smoke.wasm
