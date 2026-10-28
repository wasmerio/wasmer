#!/usr/bin/env bash
# GNU findutils 4.11.0 for WASIX v2 (asyncify: find -exec and xargs fork).
# Only find and xargs; locate/updatedb need a database and are not built.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/findutils
VER=4.11.0
TARBALL=$(fetch https://ftp.gnu.org/gnu/findutils/findutils-$VER.tar.xz \
  bfd19cb06cc71f3352d567e90284d8cdac02ac89774bbeadf0b533b0c11432fd)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# See coreutils/build.sh for the vfork cache variables.
"$SRC/configure" \
  ac_cv_func_vfork=yes ac_cv_func_vfork_works=yes \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  CFLAGS="-g -O2"

make -j"$JOBS" -C gl
make -j"$JOBS" -C lib
make -j"$JOBS" -C find find
make -j"$JOBS" -C xargs xargs
install_wasm find/find find
install_wasm xargs/xargs xargs
