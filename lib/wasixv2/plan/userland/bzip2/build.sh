#!/usr/bin/env bash
# bzip2 1.0.8 for WASIX v2 (asyncify): bzip2 (also bunzip2 and bzcat by
# argv[0]) and bzip2recover. Plain Makefile, no configure.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/bzip2
VER=1.0.8
TARBALL=$(fetch https://sourceware.org/pub/bzip2/bzip2-$VER.tar.gz \
  ab5a03176ee106d3f0fa90e381da478ddae405918153cca248e682cd0c4a2269)

# The Makefile builds in the source tree; build in a fresh extraction.
SRC=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"
cd "$SRC"
make -j"$JOBS" bzip2 bzip2recover CC="$CC" AR="$AR" RANLIB="$RANLIB" \
  CFLAGS="-g -O2 -D_FILE_OFFSET_BITS=64"
install_wasm bzip2 bzip2
install_wasm bzip2recover bzip2recover
ln -sf bzip2 "$OUT/bin/bunzip2"
ln -sf bzip2 "$OUT/bin/bzcat"
