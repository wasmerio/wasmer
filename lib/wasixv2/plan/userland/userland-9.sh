#!/usr/bin/env bash
# Static source builds; asyncify is required for every fork-capable program.
source "$(dirname "$0")/common.sh"
export BINARYEN_CORES=$JOBS OMP_NUM_THREADS=$JOBS CARGO_INCREMENTAL=0
ROOT=${USERLAND9:-$HOME/wasixv2-tools/userland-9}
OUT=$ROOT/out DOWNLOADS=$ROOT/downloads
PREFIX=$ROOT/prefix
mkdir -p "$OUT/bin" "$PREFIX"
export PATH="$ROOT/host/bin:$HOME/wasixv2-tools/host-bison/bin:$PATH"
pkg=${1:?one package required}
source_tree() {
  local entry url sum
  entry=$(grep -F "$1" "$USERLAND/userland-9.sources")
  IFS=$'\t' read -r url sum <<< "$entry"
  SRC=$ROOT/$pkg-src
  if [ ! -d "$SRC" ]; then unpack "$(fetch "$url" "$sum")" "$SRC"; fi
  cd "$SRC"
}
case $pkg in
  zlib)
    source_tree zlib-
    CHOST=wasm32 CC=wasixcc ./configure --static --prefix="$PREFIX" 
    make -j"$JOBS"
    make -j"$JOBS" install
    ;;
  sqlite)
    source_tree sqlite-autoconf-
    ./configure --host="$HOST" --build="$BUILD" --disable-shared --enable-static \
      --disable-readline --disable-load-extension CFLAGS=-O2
    make -j"$JOBS"
    install_wasm sqlite3 sqlite3
    ;;
  git)
    source_tree git-2.
    make -j"$JOBS" CC="$CC" AR="$AR" RANLIB="$RANLIB" CFLAGS=-O2 \
      uname_S=Linux uname_M=wasm32 NO_CURL=YesPlease NO_OPENSSL=YesPlease \
      NO_EXPAT=YesPlease NO_GETTEXT=YesPlease NO_TCLTK=YesPlease NO_PERL=YesPlease \
      NO_PYTHON=YesPlease NO_ICONV=YesPlease NO_INSTALL_HARDLINKS=YesPlease \
      NO_REGEX=NeedsStartEnd \
      ZLIB_PATH="$PREFIX" prefix=/usr GIT_TEST_CMP_USE_COPIED_CONTEXT=YesPlease \
      git git-upload-pack git-receive-pack git-upload-archive
    for p in git git-upload-pack git-receive-pack git-upload-archive; do install_wasm "$p" "$p"; done
    ;;
  make)
    source_tree make-4.
    ./configure --host="$HOST" --build="$BUILD" --disable-nls --without-guile CFLAGS=-O2
    make -j"$JOBS"
    install_wasm make make
    ;;
  libevent)
    source_tree libevent-
    ./configure --host="$HOST" --build="$BUILD" --prefix="$PREFIX" \
      --disable-shared --enable-static --disable-openssl --disable-samples \
      --disable-libevent-regress CFLAGS=-O2
    make -j"$JOBS"
    make -j"$JOBS" install
    ;;
  ncurses)
    source_tree ncurses-
    ./configure --host="$HOST" --build="$BUILD" --prefix="$PREFIX" \
      --without-shared --without-debug --without-cxx --without-cxx-binding \
      --without-ada --without-progs --without-tests --enable-widec \
      --with-build-cc=cc --with-termlib CFLAGS=-O2
    make -j"$JOBS"
    make -j"$JOBS" install
    ;;
  tmux)
    source_tree tmux-
    PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig" ./configure --host="$HOST" --build="$BUILD" \
      --disable-utf8proc --disable-systemd CFLAGS="-O2 -I$PREFIX/include -I$PREFIX/include/ncursesw" \
      LDFLAGS="-L$PREFIX/lib" LIBEVENT_LIBS=-levent_core LIBEVENT_CFLAGS="-I$PREFIX/include" \
      LIBTINFO_LIBS=-ltinfow LIBTINFO_CFLAGS="-I$PREFIX/include/ncursesw"
    make -j"$JOBS"
    install_wasm tmux tmux
    ;;
  rsync)
    source_tree rsync-
    ./configure --host="$HOST" --build="$BUILD" --disable-xxhash --disable-zstd \
      --disable-lz4 --disable-openssl --disable-asm --disable-md2man CFLAGS=-O2
    make -j"$JOBS"
    install_wasm rsync rsync
    ;;
  lua)
    source_tree lua-5.
    # Upstream posix enables POSIX APIs without the linux target's dlopen support.
    make -j"$JOBS" posix CC="$CC" AR="$AR rcu" RANLIB="$RANLIB" MYCFLAGS=-O2
    install_wasm src/lua lua
    install_wasm src/luac luac
    ;;
  perl)
    source_tree perl-5.
    export WASIXV2_PROBE_CC=wasixcc
    export WASIXV2_PROBE_BIN=${USERLAND_BASE:-$HOME/wasixv2-tools/userland}/out/bin
    : "${WASIXV2_PROBE_CLI:?release CLI required for actual target probes}"
    sh Configure -des -Dcc="$USERLAND/tests/probe-cc-9.py" -Dld="$USERLAND/tests/probe-cc-9.py" \
      -Dar="$AR" -Dnm="$NM" -Dranlib="$RANLIB" -Dosname=linux -Darchname=wasm32-linux \
      -Dlibc="$WASIXCC_SYSROOT_PREFIX/sysroot-wasixv2-asyncify/lib/wasm32-wasixv2/libc.a" \
      -Dlibpth="$WASIXCC_SYSROOT_PREFIX/sysroot-wasixv2-asyncify/lib/wasm32-wasixv2 $PREFIX/lib" \
      -Dloclibpth="$PREFIX/lib" -Dusrinc="$WASIXCC_SYSROOT_PREFIX/sysroot-wasixv2-asyncify/include" \
      -Dusedl=undef -Duseshrplib=false -Dusemymalloc=n -Doptimize=-O2 \
      -Dccflags="-D_GNU_SOURCE -fno-strict-aliasing" -Dcccdlflags= -Dccdlflags= -Dlddlflags= -Dldflags= -Dlibs=-lm
    make -j"$JOBS" miniperl lib/Config.pm
    install_wasm miniperl.wasm miniperl
    mkdir -p "$ROOT/perl-tests"
    cp -a t lib MANIFEST config.sh TestInit.pm "$ROOT/perl-tests/"
    ln -sf /bin/miniperl "$ROOT/perl-tests/t/perl"
    ;;
  openssl)
    source_tree openssl-
    CC="$CC" AR="$AR" RANLIB="$RANLIB" ./Configure linux-generic32 \
      no-asm no-shared no-dso no-module no-engine no-tests no-afalgeng \
      --prefix="$PREFIX" --libdir=lib -O2
    make -j"$JOBS" build_libs
    make -j"$JOBS" install_dev
    ;;
  openssh)
    source_tree openssh-
    if ! grep -q 'RAND_add("", 0, 0.0)' configure; then
      apply_patches "$SRC" "$USERLAND/openssh/patches"
    fi
    # AC_CHECK_LIB calls deflate through char deflate(void), producing a Wasm
    # signature-mismatch trap stub that Binaryen 129 cannot parse. Establish
    # the cache value by actually compiling/running the correctly typed API.
    : "${WASIXV2_PROBE_CLI:?release CLI required for actual zlib probe}"
    cat > "$ROOT/ssh-zlib-probe.c" <<'C'
#include <string.h>
#include <zlib.h>
int main(void) {
  const unsigned char input[] = "ssh-static-zlib";
  unsigned char compressed[128], output[128];
  z_stream stream = {0};
  stream.next_in = (unsigned char *)input;
  stream.avail_in = sizeof(input);
  stream.next_out = compressed;
  stream.avail_out = sizeof(compressed);
  if (deflateInit(&stream, 6) != Z_OK) return 1;
  if (deflate(&stream, Z_FINISH) != Z_STREAM_END) return 2;
  unsigned long length = stream.total_out;
  if (deflateEnd(&stream) != Z_OK) return 3;
  unsigned long output_length = sizeof(output);
  if (uncompress(output, &output_length, compressed, length) != Z_OK) return 4;
  return output_length != sizeof(input) || memcmp(input, output, sizeof(input));
}
C
    "$CC" -O2 -I"$PREFIX/include" "$ROOT/ssh-zlib-probe.c" -L"$PREFIX/lib" -lz -o "$ROOT/ssh-zlib-probe.wasm"
    "$WASIXV2_PROBE_CLI" run --compiler-threads 3 --no-tty "$ROOT/ssh-zlib-probe.wasm"
    export ac_cv_lib_z_deflate=yes
    source "$USERLAND/tests/openssl-probes-9.sh"
    ./configure --host="$HOST" --build="$BUILD" --prefix=/usr --sysconfdir=/etc/ssh \
      --with-ssl-dir="$PREFIX" --with-zlib="$PREFIX" --with-sandbox=no \
      --without-pam --without-selinux --without-kerberos5 --without-libedit \
      --without-stackprotect --without-hardening --without-retpoline --without-rpath --disable-strip CFLAGS=-O2
    make -j"$JOBS" ssh sshd sshd-session sshd-auth ssh-keygen scp sftp-server
    for p in ssh sshd sshd-session sshd-auth ssh-keygen scp sftp-server; do install_wasm "$p" "$p"; done
    ;;
  *) echo "unknown package: $pkg" >&2; exit 2;;
esac
