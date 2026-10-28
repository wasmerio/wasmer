#!/usr/bin/env bash
# LTP's shell test library for WASIX v2: testcases/lib/*.sh plus the C helpers
# they call (tst_timeout_kill, tst_sleep, tst_getconf, tst_run_shell, ...), from
# the wasix-org/ltp fork at the commit wasixv2-tests pins under upstream/ltp.
# Installs into out/ltp (an $LTPROOT: testcases/bin holds scripts and helpers).
#
# Mirrors wasixv2-tests' src/upstream/ltp.rs: the same configure, with the
# Linux UAPI headers the libc2 sysroot ships. LTP needs autotools on the host
# (run via nix shell if missing).
source "$(dirname "$0")/../common.sh"
PKG=$USERLAND/ltp-shell
COMMIT=759bb31bfa649320d24b07170756d5418498b074
TARBALL=$(fetch https://github.com/wasix-org/ltp/archive/$COMMIT.tar.gz \
  6a10c64d45f568505f5893119602810f7845d03a88a7da5589a0254ccee9a277)

if ! command -v autoreconf >/dev/null; then
  exec nix shell nixpkgs#autoconf nixpkgs#automake nixpkgs#m4 nixpkgs#libtool -c "$0" "$@"
fi

# LTP's makefiles build in the source tree; build in a fresh copy of the tarball.
SRC=$PKG/build
unpack "$TARBALL" "$SRC"
apply_patches "$SRC" "$PKG/patches"
cd "$SRC"
export WASIXCC_DISCARD_UNSUPPORTED_FLAGS=yes
make autotools
WASIXCC_AUTOCONF_WORKAROUNDS=yes ./configure \
  --host=wasm32-unknown-wasixv2 --prefix=/opt/ltp \
  CC=wasixcc AR=wasixar RANLIB=wasixranlib NM=wasixnm HOSTCC=cc
make -C lib -j"$JOBS"
make -C libs -j"$JOBS"
make -C testcases/lib -j"$JOBS"
rm -rf "$OUT/ltp"
make -C testcases/lib install DESTDIR="$OUT/ltp-root"
mv "$OUT/ltp-root/opt/ltp" "$OUT/ltp" && rm -rf "$OUT/ltp-root"
# A few self-tests of the shell library, for validation.
mkdir -p "$OUT/ltp/testcases/bin/selftest"
cp lib/newlib_tests/shell/*.sh testcases/lib/tests/*.sh "$OUT/ltp/testcases/bin/selftest/"
# The plain shell tests of testcases/commands (wc01.sh, mkdir02.sh, ...).
cp testcases/commands/*/*.sh "$OUT/ltp/testcases/bin/"
