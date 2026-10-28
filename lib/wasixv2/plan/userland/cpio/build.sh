#!/usr/bin/env bash
# GNU cpio 2.15 for WASIX v2 (asyncify).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/cpio
VER=2.15
TARBALL=$(fetch https://ftp.gnu.org/gnu/cpio/cpio-$VER.tar.bz2 \
  937610b97c329a1ec9268553fb780037bcfff0dcffe9725ebc4fd9c1aa9075db)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
"$SRC/configure" \
  ac_cv_func_vfork=yes ac_cv_func_vfork_works=yes \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  --disable-mt \
  CFLAGS="-g -O2"

make -j"$JOBS" -C gnu
make -j"$JOBS" -C lib
make -j"$JOBS" -C src cpio
install_wasm src/cpio cpio
