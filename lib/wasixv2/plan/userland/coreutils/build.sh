#!/usr/bin/env bash
# GNU coreutils 9.12 for WASIX v2 (asyncify), as one multi-call binary that
# dispatches on argv[0]; out/bin gets a symlink per program.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/coreutils
VER=9.12
TARBALL=$(fetch https://ftp.gnu.org/gnu/coreutils/coreutils-$VER.tar.xz \
  a480198559733e9b3da999e90543ac6f888a2caa544d8d664c5a1f17e528e210)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# ac_cv_func_vfork{,_works}=yes: libc2's vfork() is a macro (no linkable
# symbol), so AC_FUNC_FORK's link probe misses it and configure emits
# `#define vfork fork`, which rewrites libc2's unavailable vfork declaration in
# <unistd.h> into one for fork() and breaks every later fork() call.
"$SRC/configure" \
  ac_cv_func_vfork=yes ac_cv_func_vfork_works=yes \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  --enable-single-binary=symlinks \
  --without-gmp \
  --without-openssl \
  --without-selinux \
  --disable-libcap \
  --disable-acl --disable-xattr \
  CFLAGS="-g -O2"

make -j"$JOBS"
install_wasm src/coreutils coreutils
# One symlink per program the multi-call binary was built with.
for prog in $(sed -n 's/^single_binary_progs = //p' Makefile); do
  case $prog in '[') continue ;; ginstall) prog=install ;; esac
  ln -sf coreutils "$OUT/bin/$prog"
done
