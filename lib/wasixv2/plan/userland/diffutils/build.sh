#!/usr/bin/env bash
# GNU diffutils 3.12 for WASIX v2 (asyncify: diff3/sdiff run diff). LTP's shell
# tests compare outputs with diff and cmp.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/diffutils
VER=3.12
TARBALL=$(fetch https://ftp.gnu.org/gnu/diffutils/diffutils-$VER.tar.xz \
  7c8b7f9fc8609141fdea9cece85249d308624391ff61dedaf528fcb337727dfd)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# See coreutils/build.sh for the vfork cache variables. gnulib's strcasecmp
# check errors out when cross compiling instead of taking the guess it computes
# just before ("guessing yes" outside Solaris/Cygwin); pass that guess.
"$SRC/configure" \
  ac_cv_func_vfork=yes ac_cv_func_vfork_works=yes \
  gl_cv_func_strcasecmp_works="guessing yes" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  CFLAGS="-g -O2"

make -j"$JOBS" -C lib
make -j"$JOBS" -C src
for p in diff cmp diff3 sdiff; do install_wasm src/$p $p; done
