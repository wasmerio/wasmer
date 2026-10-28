#!/usr/bin/env bash
# GNU gzip 1.14 for WASIX v2 (asyncify). gunzip, zcat and the other z* commands
# are gzip's shell scripts, run by the userland's bash.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/gzip
VER=1.14
TARBALL=$(fetch https://ftp.gnu.org/gnu/gzip/gzip-$VER.tar.xz \
  01a7b881bd220bfdf615f97b8718f80bdfd3f6add385b993dcf6efd14e8c0ac6)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
"$SRC/configure" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  CFLAGS="-g -O2"

# doc and tests are not needed and only add build time.
make -j"$JOBS" SUBDIRS="lib ."
install_wasm gzip gzip
# The scripts start with #!/bin/sh and find gzip on PATH (bindir is /bin).
for s in gunzip zcat zcmp zdiff zgrep zegrep zfgrep zforce zless zmore znew gzexe; do
  install -m 0755 "$s" "$OUT/bin/$s"
done
