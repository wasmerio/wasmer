#!/usr/bin/env bash
# Real-program validation kit. Sources and binaries stay outside the repository.
# Run under nice -n19 taskset -c 4-7; this script builds one package at a time.
source "$(dirname "$0")/common.sh"
export BINARYEN_CORES=$JOBS OMP_NUM_THREADS=$JOBS
ROOT=${USERLAND5:-$HOME/wasixv2-tools/userland-5}
OUT=$ROOT/out
DOWNLOADS=$ROOT/downloads
mkdir -p "$OUT/bin"

checkout() {
  local name=$1 url=$2 rev=$3
  if [ ! -d "$ROOT/$name-src/.git" ]; then
    git clone "$url" "$ROOT/$name-src"
  fi
  git -C "$ROOT/$name-src" checkout --detach "$rev"
}

build_uring() {
  checkout liburing https://github.com/axboe/liburing.git e907d6a342e80b70874f93abd440b92b8a40b7bc
  cd "$ROOT/liburing-src"
  ./configure --cc="$CC" --cxx="$CXX" --use-libc
  # Build only the C library. No FFI library or C++ tests are needed.
  make -C src -j"$JOBS" CC="$CC" AR="$AR" RANLIB="$RANLIB" liburing.a
  make -C test -j"$JOBS" CC="$CC" AR="$AR" \
    nop.t read-write.t poll.t timeout.t eventfd.t socket.t \
    open-close.t statx.t unlink.t mkdir.t exec-target.t
}

build_aio() {
  checkout libaio https://pagure.io/libaio.git b8eadc9f89e8f7ab0338eacda9f98a6caea76883
  cd "$ROOT/libaio-src"
  # Static Wasm aliases for libaio's .symver directives require LLVM branch
  # wasix-org/llvm-project:wasixv2/wasm-symver. No libaio source patch is used.
  # Select an isolated build without changing the compiler for other packages.
  if [ -n "${LIBAIO_LLVM_LOCATION:-}" ]; then
    export WASIXCC_LLVM_LOCATION=$LIBAIO_LLVM_LOCATION
  fi
  make -C src -j"$JOBS" CC="$CC" AR="$AR" RANLIB="$RANLIB" ENABLE_SHARED=0
  make -C harness -j"$JOBS" CC="$CC" cases/2.p cases/3.p cases/4.p \
    cases/5.p cases/12.p cases/15.p cases/22.p
}

# A narrow upstream harness target usable while time64-only AIO waiting is
# unavailable in libaio. Its Makefile documents LIBAIO as a link-input override;
# use the real io_setup object, with no replacement implementation/source edits.
build_aio_setup() {
  checkout libaio https://pagure.io/libaio.git b8eadc9f89e8f7ab0338eacda9f98a6caea76883
  cd "$ROOT/libaio-src"
  if [ -n "${LIBAIO_LLVM_LOCATION:-}" ]; then
    export WASIXCC_LLVM_LOCATION=$LIBAIO_LLVM_LOCATION
  fi
  make -B -C src -j"$JOBS" CC="$CC" io_setup.ol
  "$AR" rcs "$ROOT/libaio-setup.a" src/io_setup.ol
  make -B -C harness -j"$JOBS" CC="$CC" LIBAIO="$ROOT/libaio-setup.a" cases/2.p
}

build_keys() {
  checkout keyutils https://git.kernel.org/pub/scm/linux/kernel/git/dhowells/keyutils.git c076dff259e99d84d3822b4d2ad7f3f66532f411
  cd "$ROOT/keyutils-src"
  make -j"$JOBS" CC="$CC" AR="$AR" NO_SOLIB=1 CFLAGS='-O2 -g -Wall' \
    LIBDIR=/lib USRLIBDIR=/usr/lib keyctl
  install_wasm keyctl keyctl
}

build_strace() {
  # wasm32 still needs a real architecture/scalar-slot port; see strace-wasm32.md.
  checkout strace https://github.com/strace/strace.git 9d7b4d1a59a20171bfad65a4eea883190ddd3791
  cd "$ROOT/strace-src"
  ./bootstrap
  ./configure --host="$HOST" --build="$BUILD" --disable-shared --enable-static \
    --without-libunwind --without-libdw --disable-mpers
  make -j"$JOBS" CC="$CC"
  install_wasm src/strace strace
}

build_util() {
  local tarball src=$ROOT/util-linux-src
  checkout libcap-ng https://github.com/stevegrubb/libcap-ng.git f5d39702622208b3ada064d7b2eaeaf1454c9bd3
  cd "$ROOT/libcap-ng-src"
  ./autogen.sh
  ./configure --host="$HOST" --build="$BUILD" --prefix="$ROOT/prefix" \
    --disable-shared --enable-static --without-python3
  make -C src -j"$JOBS"
  make -C src install
  # AC_CHECK_LIB declares char capng_apply(void), conflicting with the enum
  # argument in libcap-ng. Wasm requires matching function signatures. Verify
  # the real prototype before supplying the result to that configure probe.
  printf '#include <cap-ng.h>\nint main(void) { return capng_apply(CAPNG_SELECT_BOTH); }\n' \
    > "$ROOT/capng-link.c"
  "$CC" -I"$ROOT/prefix/include" "$ROOT/capng-link.c" \
    -L"$ROOT/prefix/lib" -lcap-ng -o "$ROOT/capng-link"
  tarball=$(fetch https://www.kernel.org/pub/linux/utils/util-linux/v2.41/util-linux-2.41.2.tar.xz \
    6062a1d89b571a61932e6fc0211f36060c4183568b81ee866cf363bce9f6583e)
  unpack "$tarball" "$src"
  mkdir -p "$ROOT/util-linux-build"
  cd "$ROOT/util-linux-build"
  "$src/configure" --host="$HOST" --build="$BUILD" --prefix=/ \
    --disable-shared --disable-nls --disable-liblastlog2 --without-python \
    --without-systemd --without-udev --without-ncurses --without-tinfo \
    --without-readline --without-libmagic --without-econf \
    CPPFLAGS="-I$ROOT/prefix/include" LDFLAGS="-L$ROOT/prefix/lib" \
    PKG_CONFIG_LIBDIR="$ROOT/prefix/lib/pkgconfig" \
    ac_cv_lib_cap_ng_capng_apply=yes CFLAGS='-g -O2'
  make -j"$JOBS" unshare nsenter setpriv dmesg
  for p in unshare nsenter setpriv dmesg; do install_wasm "$p" "$p"; done
}

build_ip() {
  local tarball
  tarball=$(fetch https://www.kernel.org/pub/linux/utils/net/iproute2/iproute2-7.3.0.tar.xz \
    5c1789d290bba048a6135ed1ab81b0402dd0fb528d1f1421765c42b1fc3d6289)
  unpack "$tarball" "$ROOT/iproute2-src"
  cd "$ROOT/iproute2-src"
  PKG_CONFIG=false ./configure --prefix=/usr
  make -j"$JOBS" SHARED_LIBS=n CC="$CC" HOSTCC="$CC_FOR_BUILD" AR="$AR" \
    CCOPTS='-g -O2' SUBDIRS='lib ip misc'
  install_wasm ip/ip ip
  install_wasm misc/ss ss
  "$CC" -O2 -g "$USERLAND/tests/repros/ss-listener.c" -o "$OUT/bin/ss-listener"
}

build_seccomp() {
  # Linux's own executable sandbox sample; no libseccomp dependency.
  local linux=${LINUX_SOURCE:-$HOME/wasixv2-tools/linux}
  "$CC" -O2 -g "$linux/samples/seccomp/dropper.c" -o "$OUT/bin/seccomp-dropper"
}

build_core() {
  "$CC" -O2 -g "$USERLAND/tests/repros/core-file.c" -o "$OUT/bin/core-file"
}

build_fuse() {
  checkout squashfuse https://github.com/vasi/squashfuse.git 775b4cc72ab47641637897f11ce0da15d5c1f115
  checkout zlib https://github.com/madler/zlib.git 51b7f2abdade71cd9bb0e7a373ef2610ec6f9daf
  checkout libfuse https://github.com/libfuse/libfuse.git ddcb1a075995500bd6c91327a1dfcee346acd075
  cd "$ROOT/zlib-src"
  ./configure --static --prefix="$ROOT/prefix"
  make -j"$JOBS"
  make install
  cat > "$ROOT/fuse-cross.ini" <<EOF
[binaries]
c = '$(command -v "$CC")'
ar = '$(command -v "$AR")'
strip = '$WASIXCC_LLVM_LOCATION/bin/llvm-strip'
pkg-config = 'pkg-config'
[host_machine]
system = 'linux'
cpu_family = 'wasm32'
cpu = 'wasm32'
endian = 'little'
[properties]
needs_exe_wrapper = true
[built-in options]
c_args = ['-O2']
EOF
  meson setup "$ROOT/libfuse-build" "$ROOT/libfuse-src" \
    --cross-file "$ROOT/fuse-cross.ini" --default-library=static \
    --prefix="$ROOT/prefix" --libdir=lib -Dutils=false -Dtests=false \
    -Dexamples=false -Ddisable-mtab=true -Ddisable-libc-symbol-version=true
  ninja -C "$ROOT/libfuse-build" -j"$JOBS"
  ninja -C "$ROOT/libfuse-build" install
  # Autoconf's unprototyped probe conflicts with zlib's four-argument API.
  printf '#include <zlib.h>\nint main(void) { unsigned long n=0; return uncompress(0, &n, 0, 0); }\n' \
    > "$ROOT/zlib-link.c"
  "$CC" -I"$ROOT/prefix/include" "$ROOT/zlib-link.c" \
    -L"$ROOT/prefix/lib" -lz -o "$ROOT/zlib-link"
  printf '#define FUSE_USE_VERSION 32\n#include <fuse_lowlevel.h>\nint main(void) { return fuse_session_loop(0); }\n' \
    > "$ROOT/fuse-link.c"
  "$CC" -D_FILE_OFFSET_BITS=64 -I"$ROOT/prefix/include/fuse3" "$ROOT/fuse-link.c" \
    -L"$ROOT/prefix/lib" -lfuse3 -pthread -o "$ROOT/fuse-link"
  cd "$ROOT/squashfuse-src"
  ./autogen.sh
  PKG_CONFIG_LIBDIR="$ROOT/prefix/lib/pkgconfig" ./configure \
    --host="$HOST" --build="$BUILD" --prefix="$ROOT/prefix" \
    --disable-shared --enable-static --disable-demo --disable-high-level \
    CPPFLAGS="-I$ROOT/prefix/include" LDFLAGS="-L$ROOT/prefix/lib" \
    --without-xz --without-lzo --without-lz4 --without-zstd \
    ac_cv_search_uncompress=-lz ac_cv_func_fuse_session_loop=yes
  make -j"$JOBS"
  install_wasm squashfuse_ll squashfuse_ll
}

case ${1:-help} in
  liburing) build_uring ;;
  libaio) build_aio ;;
  libaio-setup) build_aio_setup ;;
  keyutils) build_keys ;;
  strace) build_strace ;;
  util-linux) build_util ;;
  iproute2) build_ip ;;
  seccomp) build_seccomp ;;
  core) build_core ;;
  fuse) build_fuse ;;
  *) echo "Usage: $0 {liburing|libaio|libaio-setup|keyutils|strace|util-linux|iproute2|seccomp|core|fuse}"; exit 2 ;;
esac
