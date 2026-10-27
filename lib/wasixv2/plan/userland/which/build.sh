#!/usr/bin/env bash
# GNU which 2.25 for WASIX v2 (LTP's which01.sh and tst_cmd_available fallbacks).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/which
VER=2.25
TARBALL=$(fetch https://ftp.gnu.org/gnu/which/which-$VER.tar.gz \
  1cb83e4f702e60b8211ab5ec4c2afbab1b1dec80209456a7d2faf7584ed225ea)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# gnu17: the bundled getopt.h declares getopt() without a prototype, which
# C23 (clang's default) reads as getopt(void).
"$SRC/configure" --host="$HOST" --build="$BUILD" --prefix=/ CFLAGS="-std=gnu17 -g -O2"
make -j"$JOBS" which
install_wasm which which
