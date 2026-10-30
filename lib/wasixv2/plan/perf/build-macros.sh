#!/usr/bin/env bash
# Reuse the preserved V9/V12 sources; write only to this package's output tree.
set -euo pipefail
out=${PERF_OUT:-$HOME/wasixv2-tools/userland-v14perf}
u6=${USERLAND6:-$HOME/wasixv2-tools/userland-6}
u10=${USERLAND10:-$HOME/wasixv2-tools/userland-10}
mkdir -p "$out"/{bin,native,v1,logs}
export BINARYEN_CORES=3 OMP_NUM_THREADS=3
for config in native v2 v1; do
  case $config in
    native) compiler=cc; dest=$out/native ;;
    v2) compiler=wasixcc; dest=$out/bin; export WASIXCC_TARGET=wasm32-wasixv2 WASIXCC_WASM_EXCEPTIONS=no ;;
    v1) compiler=wasixcc; dest=$out/v1; export WASIXCC_TARGET=wasm32-wasi WASIXCC_WASM_EXCEPTIONS=no
        export WASIXCC_SYSROOT_PREFIX=${WASIX1_SYSROOT_PREFIX:-$HOME/.wasixcc/sysroot-v2026-05-12.1} ;;
  esac
  # Match the upstream V12 SQLite feature flags; omit extensions on all targets.
  if nice -n 19 taskset -c 4-7 "$compiler" -O2 -DSQLITE_THREADSAFE=1 \
    -DSQLITE_OMIT_LOAD_EXTENSION=1 -DSQLITE_ENABLE_MATH_FUNCTIONS \
    -I"$u10/sqlite-build" "$u10/sqlite-src/test/speedtest1.c" \
    "$u10/sqlite-build/sqlite3.c" -pthread -lm -o "$dest/speedtest1" \
    > "$out/logs/sqlite-$config-build.log" 2>&1; then
    echo "sqlite-$config PASS"
  else
    echo "sqlite-$config BUILD FAILED: $out/logs/sqlite-$config-build.log"
  fi
done
# Native equivalents of exactly the source versions used by userland-6.
for package in redis nginx; do
  case $package in redis) archive=redis-7.0.15.tar.gz ;; nginx) archive=nginx-1.28.0.tar.gz ;; esac
  src=$out/native-$package
  if [ ! -d "$src" ]; then
    mkdir -p "$src"
    tar -xf "$u6/downloads/$archive" -C "$src" --strip-components=1
  fi
  if [ "$package" = redis ]; then
    nice -n 19 taskset -c 4-7 make -C "$src/src" -j3 CC=cc MALLOC=libc DEBUG= \
      OPTIMIZATION=-O2 USE_SYSTEMD=no BUILD_TLS=no redis-server redis-cli redis-benchmark \
      > "$out/logs/redis-native-build.log" 2>&1
    for binary in redis-server redis-cli redis-benchmark; do cp "$src/src/$binary" "$out/native/$binary"; done
  else
    (cd "$src"; nice -n 19 taskset -c 4-7 ./configure --prefix=/tmp/perf-nginx \
      --without-http_rewrite_module --without-http_gzip_module --with-cc-opt=-O2 \
      > "$out/logs/nginx-native-configure.log" 2>&1
      nice -n 19 taskset -c 4-7 make -j3 > "$out/logs/nginx-native-build.log" 2>&1)
    cp "$src/objs/nginx" "$out/native/nginx"
  fi
done
# Existing Wasm binaries are source-built by the pinned V9/V12 recipes. Preserve them.
for binary in bash sh coreutils cat cmp mktemp cp mv rm mkdir sort uniq head wc sha256sum seq sleep grep sed awk make redis-server redis-cli redis-benchmark nginx; do
  if [ -e "$u10/bin/$binary" ]; then cp -L "$u10/bin/$binary" "$out/bin/$binary"; fi
done
sha256sum "$u6/downloads/redis-7.0.15.tar.gz" "$u6/downloads/nginx-1.28.0.tar.gz" \
  "$u10/downloads/sqlite-src-3490200.zip" > "$out/source-archives.sha256"
