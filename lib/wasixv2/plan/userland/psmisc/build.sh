#!/usr/bin/env bash
# psmisc 23.7 for WASIX v2 (asyncify): killall and fuser (LTP's fs_racer.sh and
# the sched client/server tests call killall). pstree needs a termcap library,
# which the sysroot does not have.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/psmisc
VER=23.7
TARBALL=$(fetch https://downloads.sourceforge.net/project/psmisc/psmisc/psmisc-$VER.tar.xz \
  58c55d9c1402474065adae669511c191de374b0871eec781239ab400b907c327)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# ac_cv_lib_tinfo_tgetent: configure insists on a termcap library, which only
# pstree links (TERMCAP_LIB); it is not built here.
"$SRC/configure"   ac_cv_lib_tinfo_tgetent=yes \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --disable-nls \
  CFLAGS="-g -O2"

make -j"$JOBS" src/killall src/fuser
install_wasm src/killall killall
install_wasm src/fuser fuser
