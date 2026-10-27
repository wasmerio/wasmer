#!/usr/bin/env bash
# GNU sed 4.10 for WASIX v2 (asyncify).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/sed
VER=4.10
TARBALL=$(fetch https://ftp.gnu.org/gnu/sed/sed-$VER.tar.xz \
  b8e72182b2ec96a3574e2998c47b7aaa64cc20ce000d8e9ac313cc07cecf28c7)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
"$SRC/configure" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  CFLAGS="-g -O2"

# gnulib-tests is not needed and only adds build time.
make -j"$JOBS" sed/sed
install_wasm sed/sed sed
