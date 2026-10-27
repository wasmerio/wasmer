#!/usr/bin/env bash
# GNU gawk 5.4.1 for WASIX v2 (asyncify: system(), pipes and |& fork).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/gawk
VER=5.4.1
TARBALL=$(fetch https://ftp.gnu.org/gnu/gawk/gawk-$VER.tar.xz \
  07f6f7342b7febe4313fc2c2542ad93d64fe20ad8717200109f105a826f5fd37)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# --disable-extensions: the bundled extensions are dlopen()ed shared objects.
"$SRC/configure" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  --disable-extensions \
  --disable-mpfr \
  --without-readline \
  CFLAGS="-g -O2"

make -j"$JOBS" -C support
make -j"$JOBS" gawk
install_wasm gawk gawk
ln -sf gawk "$OUT/bin/awk"
