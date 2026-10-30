#!/usr/bin/env bash
# Isolated static upstream-suite builds; inherited source archives remain intact.
source "$(dirname "$0")/common.sh"
export BINARYEN_CORES=$JOBS OMP_NUM_THREADS=$JOBS CARGO_INCREMENTAL=0
ROOT=${USERLAND10:-$HOME/wasixv2-tools/userland-10}
BASE=${USERLAND_BASE:-$HOME/wasixv2-tools/userland}
mkdir -p "$ROOT/bin"
pkg=${1:?coreutils or bash}
mkdir -p "$ROOT/$pkg-build"
cd "$ROOT/$pkg-build"
case $pkg in
 coreutils)
  "$ROOT/coreutils-src/configure" --host="$HOST" --build="$BUILD" --prefix=/ \
   --disable-nls --enable-single-binary=symlinks --without-gmp --without-openssl \
   --without-selinux --disable-libcap --disable-acl --disable-xattr CFLAGS=-O2
  make -j"$JOBS"
  install -m755 src/coreutils "$ROOT/bin/coreutils"
  for p in $(sed -n 's/^single_binary_progs = //p' Makefile); do
   case $p in '[') continue ;; ginstall) p=install ;; esac
   ln -sf coreutils "$ROOT/bin/$p"
  done
  ;;
 bash)
  "$ROOT/bash-src/configure" --host="$HOST" --build="$BUILD" --prefix=/ \
   --without-bash-malloc --disable-nls --enable-readline --with-included-readline \
   bash_cv_dev_fd=standard bash_cv_termcap_lib=gnutermcap bash_cv_getenv_redef=no \
   CFLAGS='-std=gnu17 -O2' CPPFLAGS=-DNEED_EXTERN_PC
  make -j"$JOBS" CC_FOR_BUILD="$CC_FOR_BUILD -std=gnu17"
  for p in recho zecho printenv xcase; do
   "$CC" -O2 "$ROOT/bash-src/support/$p.c" -o "$p"
  done
  install -m755 bash "$ROOT/bin/bash"
  ;;
 *) exit 2;;
esac
