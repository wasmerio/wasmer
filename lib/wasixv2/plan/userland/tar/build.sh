#!/usr/bin/env bash
# GNU tar 1.35 for WASIX v2 (asyncify: -z/-j/-J run the compressor as a child).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/tar
VER=1.35
TARBALL=$(fetch https://ftp.gnu.org/gnu/tar/tar-$VER.tar.xz \
  4d62ff37342ec7aed748535323930c7cf94acf71c3591882b26a7ea50f3edc16)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# ac_cv_func_vfork*: as in coreutils (libc2's vfork() is a macro).
"$SRC/configure" \
  ac_cv_func_vfork=yes ac_cv_func_vfork_works=yes \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  --without-selinux --without-posix-acls --without-xattrs \
  CFLAGS="-g -O2"

make -j"$JOBS" -C gnu
make -j"$JOBS" -C lib
make -j"$JOBS" -C src tar
install_wasm src/tar tar
