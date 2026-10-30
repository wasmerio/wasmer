#!/usr/bin/env bash
# Isolated static upstream-suite builds; inherited source archives remain intact.
source "$(dirname "$0")/common.sh"
export BINARYEN_CORES=$JOBS OMP_NUM_THREADS=$JOBS CARGO_INCREMENTAL=0
ROOT=${USERLAND10:-$HOME/wasixv2-tools/userland-10}
BASE=${USERLAND_BASE:-$HOME/wasixv2-tools/userland}
mkdir -p "$ROOT/bin"
pkg=${1:?one suite build package required}
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
  : "${WASIXV2_PROBE_CLI:?release CLI needed for actual configure probes}"
  "$CC" -O2 "$USERLAND/tests/probes/wait-status-10.c" -o "$ROOT/wait-status.wasm"
  bash_cv_wexitstatus_offset=$("$WASIXV2_PROBE_CLI" run --compiler-threads 3 --no-tty "$ROOT/wait-status.wasm")
  [[ $bash_cv_wexitstatus_offset =~ ^[0-9]+$ ]]
  export bash_cv_wexitstatus_offset
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
 tcl|tcl-nothreads)
  : "${WASIXV2_PROBE_CLI:?release CLI needed for actual configure probes}"
  "$CC" -O2 -fno-builtin "$USERLAND/tests/probes/tcl-compat-10.c" -o "$ROOT/tcl-compat.wasm"
  "$WASIXV2_PROBE_CLI" run --compiler-threads 3 --no-tty "$ROOT/tcl-compat.wasm"
  export ac_cv_func_memcmp_working=yes tcl_cv_strstr_unbroken=ok tcl_cv_strtoul_unbroken=ok
  TPREFIX=$ROOT/prefix
  THREADS=--enable-threads
  if [ "$pkg" = tcl-nothreads ]; then TPREFIX=$ROOT/prefix-tcl-nothreads; THREADS=--disable-threads; fi
  "$ROOT/tcl-src/unix/configure" --host="$HOST" --build="$BUILD" \
   --prefix="$TPREFIX" --disable-shared --disable-load "$THREADS" CFLAGS=-O2
  make clean
  make -j"$JOBS" binaries libraries
  make -j"$JOBS" install-binaries install-libraries install-headers install-msgs install-tzdata
  install -m755 tclsh "$ROOT/bin/tclsh"
  ;;
 sqlite)
  ZPREFIX=${USERLAND9_PREFIX:-$HOME/wasixv2-tools/userland-9/prefix}
  TCLPREFIX=${TCL_CONFIG_PREFIX:-$ROOT/prefix}
  "$ROOT/sqlite-src/configure" --host="$HOST" --build="$BUILD" \
   --disable-shared --disable-load-extension --disable-readline --with-tclsh=/usr/bin/tclsh8.6 \
   CFLAGS="-O2 -I$ZPREFIX/include" LDFLAGS="-L$ZPREFIX/lib -lz"
  make -j"$JOBS" testfixture sqlite3 TCL_CONFIG_SH="$TCLPREFIX/lib/tclConfig.sh" \
   TCLLIBDIR="$TCLPREFIX/lib"
  install -m755 testfixture "$ROOT/bin/testfixture"
  install -m755 sqlite3 "$ROOT/bin/sqlite3"
  ;;
 busybox)
  cd "$ROOT/busybox-src"
  make -j"$JOBS" HOSTCC=cc defconfig
  sed -i 's/^CONFIG_HWCLOCK=y/# CONFIG_HWCLOCK is not set/; s/^CONFIG_TC=y/# CONFIG_TC is not set/; s/^CONFIG_SHA1_HWACCEL=y/# CONFIG_SHA1_HWACCEL is not set/; s/^CONFIG_SHA256_HWACCEL=y/# CONFIG_SHA256_HWACCEL is not set/; s/^CONFIG_STATIC=.*/CONFIG_STATIC=y/; s/^CONFIG_EXTRA_CFLAGS=.*/CONFIG_EXTRA_CFLAGS="-O2"/' .config
  export WASIXV2_BUSYBOX_CC=$CC
  make -j"$JOBS" CC="$USERLAND/tests/busybox-cc.py" HOSTCC=cc AR="$AR" \
   LD="$USERLAND/tests/busybox-ld.py" CROSS_COMPILE= SKIP_STRIP=y busybox
  install -m755 busybox "$ROOT/bin/busybox"
  ;;
 make)
  "$ROOT/make-src/configure" --host="$HOST" --build="$BUILD" \
   --disable-nls --without-guile CFLAGS=-O2
  make -j"$JOBS"
  install -m755 make "$ROOT/bin/make"
  ;;
 perl)
  : "${WASIXV2_PROBE_CLI:?release CLI needed for actual target probes}"
  # Keep the earlier diagnostic tree intact; Configure reuses stale config.sh
  # lists and derived installation paths even when onlyextensions/prefix change.
  PSRC=$ROOT/perl-static-src
  if [ ! -d "$PSRC" ]; then
   mkdir -p "$PSRC"
   tar -xf "$HOME/wasixv2-tools/userland-9/downloads/perl-5.40.2.tar.xz" \
    -C "$PSRC" --strip-components=1
  fi
  cd "$PSRC"
  export WASIXV2_PROBE_CC=wasixcc WASIXV2_PROBE_BIN="$ROOT/bin" WASIXV2_PROBE_ROOT="$PSRC"
  sysroot="$WASIXCC_SYSROOT_PREFIX/sysroot-wasixv2-asyncify"
  sh Configure -des -Dcc="$USERLAND/tests/probe-cc-10.py" -Dld="$USERLAND/tests/probe-cc-10.py" \
   -Dar=wasixar -Dnm=wasixnm -Dranlib=wasixranlib -Dosname=linux -Darchname=wasm32-linux \
   -Dprefix="$ROOT/prefix-perl" \
   -Dlibc="$sysroot/lib/wasm32-wasixv2/libc.a" \
   -Dlibpth="$sysroot/lib/wasm32-wasixv2" -Dusrinc="$sysroot/include" \
   -Donlyextensions="attributes Cwd Fcntl File/Glob IO List/Util POSIX re" \
   -Dstatic_ext="attributes Cwd Fcntl File/Glob IO List/Util POSIX re" \
   -Dusedl=undef -Duseshrplib=false -Dusemymalloc=n -Doptimize=-O2 \
   -Dccflags="-D_GNU_SOURCE -fno-strict-aliasing" -Dcccdlflags= -Dccdlflags= -Dlddlflags= -Dldflags= -Dlibs=-lm
  rm -f miniperl miniperl.wasm
  make -j"$JOBS" miniperl lib/Config.pm
  # The same-version native miniperl is a build-time generator, like SQLite's
  # native Tcl. Errno and static XS Makefiles must invoke the host compiler.
  # Configure's actual-target probes and the installed Perl remain Wasm.
  : "${PERL_FOR_BUILD:=$HOME/wasixv2-tools/userland-9/perl-native/miniperl}"
  "$PERL_FOR_BUILD" -e 'die "need Perl 5.40.2" unless $^V eq v5.40.2'
  cp miniperl miniperl-target-launcher
  rm miniperl
  cp "$PERL_FOR_BUILD" miniperl
  make -j"$JOBS" all
  make -j"$JOBS" install.perl
  if [ -L "$ROOT/bin/perl" ]; then rm "$ROOT/bin/perl"; fi
  install -m755 perl.wasm "$ROOT/bin/perl"
  install -m755 perl.wasm "$ROOT/prefix-perl/bin/perl"
  ;;
 *) exit 2;;
esac
