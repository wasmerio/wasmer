#!/usr/bin/env bash
# procps-ng 4.0.5 for WASIX v2 (asyncify): ps, sysctl, free, pgrep, pkill,
# pidof, kill, pmap, pwdx, uptime, vmstat (all read /proc). Without ncurses:
# no top, watch or slabtop. Static libproc2.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/procps-ng
VER=4.0.5
TARBALL=$(fetch https://downloads.sourceforge.net/project/procps-ng/Production/procps-ng-$VER.tar.xz \
  c2e6d193cc78f84cd6ddb72aaf6d5c6a9162f0470e5992092057f5ff518562fa)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

PROGS=(ps/pscommand sysctl free pgrep pkill pidof kill pmap pwdx uptime vmstat)

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
"$SRC/configure" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-shared --disable-nls \
  --without-ncurses --without-systemd --without-elogind \
  --disable-w --disable-pidwait \
  CFLAGS="-g -O2"

make -j"$JOBS" "${PROGS[@]/#/src/}"
for p in "${PROGS[@]}"; do
  case $p in ps/pscommand) install_wasm src/ps/pscommand ps ;; *) install_wasm "src/$p" "$p" ;; esac
done
