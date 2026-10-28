#!/usr/bin/env bash
# util-linux 2.41.2 for WASIX v2 (asyncify): the programs LTP's shell tests
# call (mount, umount, mountpoint, findmnt, unshare, nsenter, setsid, taskset,
# renice, uuidgen, ipcs, ipcrm, hexdump, flock, getopt). Static libraries only
# (no shared objects on wasixv2); each program is its own module.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/util-linux
VER=2.41.2
TARBALL=$(fetch https://www.kernel.org/pub/linux/utils/util-linux/v${VER%.*}/util-linux-$VER.tar.xz \
  6062a1d89b571a61932e6fc0211f36060c4183568b81ee866cf363bce9f6583e)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

PROGS=(mount umount mountpoint findmnt unshare nsenter setsid taskset renice
  uuidgen ipcs ipcrm hexdump flock getopt)

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# Configure's default program set (--disable-all-programs would also turn off
# findmnt, setsid, renice, flock and getopt, which have no --enable switch);
# make builds only PROGS. The external libraries util-linux can use are not in
# the sysroot.
"$SRC/configure" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-shared --disable-nls \
  --disable-liblastlog2 \
  --without-python --without-systemd --without-udev --without-ncurses \
  --without-tinfo --without-readline --without-cap-ng --without-libmagic \
  --without-econf \
  CFLAGS="-g -O2"

make -j"$JOBS" "${PROGS[@]}"
for p in "${PROGS[@]}"; do install_wasm "$p" "$p"; done
