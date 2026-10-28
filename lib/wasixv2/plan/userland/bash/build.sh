#!/usr/bin/env bash
# GNU bash 5.3 (+ official patches 001-020) for WASIX v2, asyncify configuration.
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/bash
VER=5.3
TARBALL=$(fetch https://ftp.gnu.org/gnu/bash/bash-$VER.tar.gz \
  0d5cd86965f869a26cf64f4b71be7b96f90a3ba8b3d74e27e8e9d9d5550f31ba)

SRC=$PKG/src
BLD=$PKG/build
unpack "$TARBALL" "$SRC"
# Official upstream patch levels (vendored, checksums in upstream.sha256).
(cd "$PKG/patches/upstream" && sha256sum -c --quiet ../upstream.sha256)
for p in "$PKG"/patches/upstream/bash53-*; do
  patch -d "$SRC" -p0 --batch --quiet < "$p"
done
apply_patches "$SRC" "$PKG/patches"

rm -rf "$BLD" && mkdir -p "$BLD" && cd "$BLD"
# Cache variables: configure cannot run wasm test programs while cross compiling.
#  - bash_cv_dev_fd: the kernel provides /dev/fd/N.
#  - bash_cv_termcap_lib=gnutermcap: use bash's bundled termcap for readline.
#  - bash_cv_getenv_redef=no: bash's getenv.o would redefine libc's
#    getenv/setenv/putenv, which wasm-ld rejects as duplicates.
"$SRC/configure" \
  --host="$HOST" --build="$BUILD" \
  --prefix=/ \
  --without-bash-malloc \
  --disable-nls \
  --enable-readline --with-included-readline \
  bash_cv_dev_fd=standard \
  bash_cv_termcap_lib=gnutermcap \
  bash_cv_getenv_redef=no \
  CFLAGS="-std=gnu17 -g -O2"

make -j"$JOBS" CC_FOR_BUILD="$CC_FOR_BUILD -std=gnu17"
install_wasm bash bash
ln -sf bash "$OUT/bin/sh"
