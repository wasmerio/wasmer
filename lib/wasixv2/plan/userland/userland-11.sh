#!/usr/bin/env bash
# Source-built static programs, following userland-9.sh conventions.
source "$(dirname "$0")/common.sh"
ROOT=${USERLAND11:-$HOME/wasixv2-tools/userland-11}
OUT=$ROOT/out PREFIX=$ROOT/prefix DOWNLOADS=$ROOT/downloads
export BINARYEN_CORES=$JOBS OMP_NUM_THREADS=$JOBS CARGO_INCREMENTAL=0
export PATH="$HOME/wasixv2-tools/host-bison/bin:$PATH"
pkg=${1:?package}
source_tree() {
  local entry url sum
  entry=$(grep -F "$1" "$USERLAND/userland-11.sources")
  IFS=$'\t' read -r url sum <<< "$entry"
  SRC=$ROOT/$pkg-src
  if [ ! -d "$SRC" ]; then unpack "$(fetch "$url" "$sum")" "$SRC"; fi
  cd "$SRC"
}
mkdir -p "$OUT/bin" "$PREFIX"
case $pkg in
openssl)
  source_tree openssl-
  CC=$CC AR=$AR RANLIB=$RANLIB ./Configure linux-generic32 no-asm no-shared no-dso no-module no-engine no-tests no-afalgeng --prefix="$PREFIX" --libdir=lib -O2
  make -j"$JOBS" build_sw
  make -j"$JOBS" install_dev
  install_wasm apps/openssl openssl
  ;;
nginx)
  source_tree nginx-
  export WASIXV2_PROBE_CC=$CC
  : "${WASIXV2_PROBE_CLI:?release CLI}"
  ./configure --crossbuild=Linux:7.2.0:wasm32 --with-cc="$USERLAND/tests/nginx-cc.py" --prefix=/tmp/nginx --without-http_rewrite_module --without-http_gzip_module --with-http_ssl_module --with-cc-opt="-O2 -I$PREFIX/include -Wno-error=sign-compare" --with-ld-opt="-static -L$PREFIX/lib"
  make -j"$JOBS"
  install_wasm objs/nginx nginx
  ;;
curl)
  source_tree curl-
  source "$USERLAND/tests/openssl-probes-11.sh"
  PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig" ./configure --host="$HOST" --build="$BUILD" --disable-shared --enable-static --with-openssl="$PREFIX" --without-libpsl --without-zlib --without-zstd --without-brotli --disable-ldap --disable-ldaps --without-libidn2 --without-librtmp --without-libssh2 --disable-manual CFLAGS=-O2 LDFLAGS="-static -L$PREFIX/lib" LIBS='-lssl -lcrypto'
  make -j"$JOBS"
  install_wasm src/curl curl
  ;;
ffmpeg)
  source_tree ffmpeg-
  ./configure --enable-cross-compile --target-os=linux --arch=wasm32 --cc="$CC" --ar="$AR" --ranlib="$RANLIB" --nm="$NM" --disable-shared --enable-static --disable-asm --disable-x86asm --disable-autodetect --disable-doc --disable-debug --disable-network --disable-hwaccels --disable-vulkan --disable-ffplay --disable-ffprobe --disable-everything --enable-indev=lavfi --enable-protocol=file,pipe --enable-demuxer=matroska,mpegts --enable-muxer=matroska,mpegts,null,framehash --enable-decoder=ffv1,pcm_s16le,mpeg2video,mp2 --enable-encoder=ffv1,pcm_s16le,mpeg2video,mp2 --enable-filter=testsrc2,sine,format,aformat,aresample,anull,null --extra-cflags=-O2
  make -j"$JOBS" ffmpeg
  install_wasm ffmpeg ffmpeg
  ;;
ncurses)
  source_tree ncurses-
  ./configure --host="$HOST" --build="$BUILD" --prefix="$PREFIX" --without-shared --without-debug --without-cxx --without-cxx-binding --without-ada --without-progs --without-tests --enable-widec --with-build-cc=cc --with-termlib CFLAGS=-O2
  make -j"$JOBS"
  make -j"$JOBS" install
  ;;
vim)
  source_tree v9.1.
  # Vim's configure executes several required probes. Its compiler launcher
  # runs real Wasm conftests; normal program outputs remain unchanged.
  export WASIXV2_PROBE_CC=$CC WASIXV2_PROBE_ROOT=$SRC WASIXV2_PROBE_BIN=$HOME/wasixv2-tools/userland/out/bin
  chmod +x "$USERLAND/tests/probe-cc-11.py"
  CC="$USERLAND/tests/probe-cc-11.py" ./configure --host="$HOST" --build="$HOST" --with-features=normal --with-tlib=tinfow --disable-gui --without-x --disable-channel --disable-terminal --disable-netbeans --disable-gpm --disable-nls --disable-acl --disable-selinux --disable-xsmp --disable-pythoninterp --disable-python3interp --disable-luainterp --disable-perlinterp --disable-rubyinterp --disable-tclinterp CFLAGS="-O2 -I$PREFIX/include -I$PREFIX/include/ncursesw" LDFLAGS="-static -L$PREFIX/lib"
  make -j"$JOBS"
  install_wasm src/vim vim
  ;;
htop)
  source_tree htop-
  PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig" ./configure --host="$HOST" --build="$BUILD" --disable-unicode --disable-sensors --disable-libnl --disable-capabilities CFLAGS="-O2 -I$PREFIX/include -I$PREFIX/include/ncursesw" LDFLAGS="-static -L$PREFIX/lib" LIBS='-lncursesw -ltinfow'
  make -j"$JOBS"
  install_wasm htop htop
  ;;
python-native)
  source_tree Python-
  cat > Modules/Setup.local <<'NATIVE'
*static*
_struct _struct.c
_posixsubprocess _posixsubprocess.c
select selectmodule.c
math mathmodule.c -lm
_random _randommodule.c
_sha2 sha2module.c -I$(srcdir)/Modules/_hacl/include Modules/_hacl/libHacl_Hash_SHA2.a
binascii binascii.c
unicodedata unicodedata.c
NATIVE
  CC=cc AR=ar RANLIB=ranlib NM=nm ./configure --prefix="$ROOT/host-python" --without-ensurepip --disable-test-modules --disable-shared CFLAGS=-O2
  make -j"$JOBS" python pybuilddir.txt
  # Use its build-tree standard library; never build optional native extensions.
  mkdir -p "$ROOT/host-python/bin"
  ln -sfn "$SRC/python" "$ROOT/host-python/bin/python3.13"
  ;;
python)
  source_tree Python-
  # No optional FFI or shared-only test extensions are built or imported.
  cat > Modules/Setup.local <<'SETUP'
*disabled*
_ctypes _ctypes_test _tkinter _testimportmultiple _testmultiphase _testsinglephase _testexternalinspection xxlimited xxlimited_35
SETUP
  : "${WASIXV2_PROBE_CLI:?release CLI}"
  "$CC" "$USERLAND/tests/probes/python-devices-11.c" -o "$ROOT/python-devices.wasm"
  "$WASIXV2_PROBE_CLI" run --compiler-threads 3 --no-tty "$ROOT/python-devices.wasm" > "$ROOT/python-site"
  CONFIG_SITE="$ROOT/python-site" PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig" ./configure --host="$HOST" --build="$BUILD" --with-build-python="$ROOT/host-python/bin/python3.13" --prefix=/python --disable-shared --without-ensurepip --with-pkg-config=no --enable-ipv6 --with-openssl="$PREFIX" CFLAGS='-O2 -fno-strict-aliasing' CPPFLAGS="-I$PREFIX/include" LDFLAGS="-static -L$PREFIX/lib"
  # Upstream's wasm32 profile emits static stdlib entries. Keep the complete
  # detected module list in Setup.local, before configure regenerates Makefile.
  cat Modules/Setup.stdlib >> Modules/Setup.local
  ./config.status
  make -j"$JOBS" python pybuilddir.txt
  cp "$(cat pybuilddir.txt)"/_sysconfigdata*.py Lib/
  install_wasm python python3
  ;;
*) echo "unknown package: $pkg" >&2; exit 2;;
esac
