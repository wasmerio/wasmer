#!/usr/bin/env bash
# GNU grep 3.12 for WASIX v2 (asyncify). Built without PCRE (-P).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/grep
VER=3.12
TARBALL=$(fetch https://ftp.gnu.org/gnu/grep/grep-$VER.tar.xz \
  2649b27c0e90e632eadcd757be06c6e9a4f48d941de51e7c0f83ff76408a07b9)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
"$SRC/configure" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  --disable-perl-regexp \
  CFLAGS="-g -O2"

# gnulib-tests is not needed and only adds build time.
make -j"$JOBS" SUBDIRS="lib src"
install_wasm src/grep grep
