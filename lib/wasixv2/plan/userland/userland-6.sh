#!/usr/bin/env bash
# One source build per invocation, using the asyncify sysroot for fork.
source "$(dirname "$0")/common.sh"
export BINARYEN_CORES=$JOBS OMP_NUM_THREADS=$JOBS CARGO_INCREMENTAL=0
ROOT=${USERLAND6:-$HOME/wasixv2-tools/userland-6}
OUT=$ROOT/out DOWNLOADS=$ROOT/downloads
mkdir -p "$OUT/bin" "$ROOT/prefix"
pkg=${1:?one package required}
source_tree() {
  local pattern=$1 entry url sum
  entry=$(grep -F "$pattern" "$USERLAND/userland-6.sources")
  IFS=$'\t' read -r url sum <<< "$entry"
  SRC=$ROOT/$pkg-src
  if [ ! -d "$SRC" ]; then unpack "$(fetch "$url" "$sum")" "$SRC"; fi
  cd "$SRC"
}
autoconf_build() {
  ./configure --host="$HOST" --build="$BUILD" --disable-nls "$@" CFLAGS='-O2 -g'
  make -j"$JOBS"
}
case $pkg in
  host-flex)
    source_tree flex-
    CC=cc CXX=c++ AR=ar RANLIB=ranlib NM=nm ./configure --prefix="$ROOT/host" --disable-nls --disable-shared
    make -j"$JOBS" CC=cc CXX=c++ AR=ar RANLIB=ranlib NM=nm
    make -j"$JOBS" CC=cc CXX=c++ AR=ar RANLIB=ranlib NM=nm install
    ;;
  dash)
    source_tree dash_
    autoconf_build --with-libedit=no
    install_wasm src/dash dash
    ;;
  redis)
    source_tree redis-7.0.
    make -j"$JOBS" CC="$CC" AR="$AR" RANLIB="$RANLIB" MALLOC=libc \
      DEBUG= OPTIMIZATION=-O2 USE_SYSTEMD=no BUILD_TLS=no uname_M=wasm32 \
      redis-server redis-cli redis-benchmark
    for p in redis-server redis-cli redis-benchmark; do install_wasm "src/$p" "$p"; done
    ;;
  postgres)
    source_tree postgresql-
    ./configure --host="$HOST" --build="$BUILD" --prefix=/pg \
      --without-icu --without-readline --without-zlib --without-lz4 --without-zstd \
      --without-openssl CFLAGS='-O2 -g'
    make -j"$JOBS"
    make -j"$JOBS" DESTDIR="$ROOT/postgres-install" install
    cp -a "$ROOT/postgres-install/pg/bin/." "$OUT/bin/"
    ;;
  curl)
    source_tree curl-
    autoconf_build --disable-shared --enable-static --without-ssl --without-libpsl \
      --without-zlib --without-zstd --without-brotli --disable-ldap --disable-ldaps
    install_wasm src/curl curl
    ;;
  wget)
    source_tree wget-
    autoconf_build --without-ssl --without-libpsl --without-libidn2 \
      --disable-ntlm --disable-iri --disable-pcre --disable-pcre2 --without-zlib
    install_wasm src/wget wget
    ;;
  socat)
    source_tree socat_
    autoconf_build --disable-openssl --disable-readline
    install_wasm socat socat
    ;;
  busybox)
    source_tree busybox-
    make -j"$JOBS" HOSTCC=cc defconfig
    # Kconfig options, not source substitutions. Standalone applets need fork.
    sed -i 's/^CONFIG_STATIC=.*/CONFIG_STATIC=y/; s/^CONFIG_EXTRA_CFLAGS=.*/CONFIG_EXTRA_CFLAGS="-O2"/' .config
    make -j"$JOBS" CC="$CC" HOSTCC=cc AR="$AR" \
      LD="$USERLAND/tests/busybox-ld.py" CROSS_COMPILE= SKIP_STRIP=y busybox
    install_wasm busybox busybox
    ;;
  libfuse)
    if [ ! -d "$ROOT/libfuse-src" ]; then
      git clone https://github.com/libfuse/libfuse.git "$ROOT/libfuse-src"
      git -C "$ROOT/libfuse-src" checkout --detach ddcb1a075995500bd6c91327a1dfcee346acd075
    fi
    cat > "$ROOT/fuse-cross.ini" <<CROSS
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
CROSS
    meson setup "$ROOT/libfuse-build" "$ROOT/libfuse-src" \
      --cross-file "$ROOT/fuse-cross.ini" --default-library=static \
      --prefix="$ROOT/prefix" --libdir=lib -Dutils=false -Dtests=false \
      -Dexamples=false -Ddisable-mtab=true -Ddisable-libc-symbol-version=true
    ninja -C "$ROOT/libfuse-build" -j"$JOBS"
    ninja -C "$ROOT/libfuse-build" install
    ;;
  fuse-overlayfs)
    source_tree /v1.15.
    ./autogen.sh
    PKG_CONFIG_LIBDIR="$ROOT/prefix/lib/pkgconfig" autoconf_build
    install_wasm fuse-overlayfs fuse-overlayfs
    ;;
  sshfs)
    source_tree sshfs-
    PKG_CONFIG_LIBDIR="$ROOT/prefix/lib/pkgconfig" meson setup "$ROOT/sshfs-build" . \
      --cross-file "$ROOT/fuse-cross.ini" --default-library=static \
      --prefix="$ROOT/prefix"
    ninja -C "$ROOT/sshfs-build" -j"$JOBS"
    install_wasm "$ROOT/sshfs-build/sshfs" sshfs
    ;;
  glib)
    source_tree glib-
    PKG_CONFIG_LIBDIR="$ROOT/prefix/lib/pkgconfig" meson setup "$ROOT/glib-build" . \
      --cross-file "$ROOT/fuse-cross.ini" --default-library=static \
      --prefix="$ROOT/prefix" --libdir=lib -Dintrospection=disabled \
      -Dtests=false -Dlibmount=disabled -Dnls=disabled --wrap-mode=nofallback
    ninja -C "$ROOT/glib-build" -j"$JOBS"
    ninja -C "$ROOT/glib-build" install
    ;;
  nginx)
    source_tree nginx-
    # Upstream runs configure probes directly. The compiler wrapper leaves
    # normal outputs as Wasm and makes only objs/autotest a CLI launcher.
    export WASIXV2_PROBE_CC=$CC WASIXV2_PROBE_CLI=${WASMER:?release CLI required}
    ./configure --crossbuild=Linux:7.2.0:wasm32 --with-cc="$USERLAND/tests/nginx-cc.py" \
      --prefix=/work/nginx --without-http_rewrite_module --without-http_gzip_module \
      --with-cc-opt='-O2 -g -Wno-error=sign-compare'
    make -j"$JOBS"
    install_wasm objs/nginx nginx
    ;;
  *) echo "Unknown package: $pkg" >&2; exit 2 ;;
esac
