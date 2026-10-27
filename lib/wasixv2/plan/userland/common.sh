# Shared helpers for the WASIX v2 userland build scripts. Sourced by <pkg>/build.sh.
#
# Every package is built out of tree against the libc2 asyncify sysroot
# (WASIXCC_WASM_EXCEPTIONS=no): fork() and setjmp/longjmp both need the asyncify
# pass, and using one configuration for all packages keeps them uniform.
set -euo pipefail

USERLAND=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DOWNLOADS=$USERLAND/downloads
OUT=$USERLAND/out
# Shared machine: keep make parallelism low (orchestrator rule 12).
JOBS=${JOBS:-3}

# shellcheck disable=SC1091
source "${WASIXV2_ENV:-$USERLAND/../env.sh}"
export WASIXCC_TARGET=wasm32-wasixv2
export WASIXCC_WASM_EXCEPTIONS=no

export CC=wasixcc CXX=wasixcc++ AR=wasixar RANLIB=wasixranlib NM=wasixnm
# Programs that run on the build host during the build (bash's mkbuiltins etc).
export CC_FOR_BUILD=${CC_FOR_BUILD:-cc}
HOST=wasm32-unknown-wasi
BUILD=$(cc -dumpmachine)
export HOST BUILD

# fetch <url> <sha256>: download into downloads/ once and verify the checksum.
fetch() {
  local url=$1 sum=$2 f
  f=$DOWNLOADS/$(basename "$url")
  mkdir -p "$DOWNLOADS"
  if [ ! -f "$f" ]; then
    curl -fL --retry 3 -o "$f.part" "$url"
    mv "$f.part" "$f"
  fi
  echo "$sum  $f" | sha256sum -c --quiet - || { echo "checksum mismatch: $f" >&2; exit 1; }
  printf '%s\n' "$f"
}

# unpack <tarball> <dest-dir>: fresh extraction (strips the top-level dir).
unpack() {
  rm -rf "$2"
  mkdir -p "$2"
  tar -xf "$1" -C "$2" --strip-components=1
}

# apply_patches <src-dir> <patch-dir> [-pN]: apply every *.patch in order.
apply_patches() {
  local src=$1 dir=$2 strip=${3:--p1} p
  [ -d "$dir" ] || return 0
  for p in "$dir"/*.patch; do
    [ -e "$p" ] || continue
    echo "applying $(basename "$p")"
    patch -d "$src" "$strip" --forward --batch < "$p"
  done
}

# install_wasm <built-binary> <name>: copy a linked module into out/bin.
install_wasm() {
  mkdir -p "$OUT/bin"
  install -m 0755 "$1" "$OUT/bin/$2"
}

# gnulib_musl_port <getlocalename_l-unsafe.c>: gnulib recognises musl only via
# __linux__, which the wasixv2 target does not define, and #errors on
# "unknown" platforms. libc2 is musl, so take gnulib's musl arm
# (nl_langinfo_l(NL_LOCALE_NAME(cat), loc)) on __wasixv2__ as well.
gnulib_musl_port() {
  local f=$1
  grep -q '__wasixv2__' "$f" && return 0
  sed -i \
    -e 's/(defined __linux__ && HAVE_LANGINFO_H)/((defined __linux__ || defined __wasixv2__) \&\& HAVE_LANGINFO_H)/' \
    -e 's/^#elif defined __linux__ && HAVE_LANGINFO_H && defined NL_LOCALE_NAME/#elif (defined __linux__ || defined __wasixv2__) \&\& HAVE_LANGINFO_H \&\& defined NL_LOCALE_NAME/' \
    "$f"
  [ "$(grep -c '__wasixv2__' "$f")" -eq 2 ] || { echo "gnulib_musl_port: $f did not match" >&2; exit 1; }
}
