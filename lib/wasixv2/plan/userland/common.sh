# Shared helpers for the WASIX v2 userland build scripts. Sourced by <pkg>/build.sh.
#
# Every package is built out of tree against the libc2 asyncify sysroot
# (WASIXCC_WASM_EXCEPTIONS=no): fork() and setjmp/longjmp both need the asyncify
# pass, and using one configuration for all packages keeps them uniform. The
# target is Linux to the sources (clang defines __linux__ for wasm32-wasixv2),
# and the sysroot ships the Linux UAPI headers.
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
# Configure's host: Linux with musl, which is what the target is to the sources
# and what libc2 derives from. Cross-compiling configure scripts can't run test
# programs and fall back to per-OS guesses: gnulib's for linux-musl match the
# target, its pessimistic ones for an unknown OS replaced working libc
# functions, and util-linux builds mount, unshare etc. only for Linux hosts.
HOST=wasm32-unknown-linux-musl
BUILD=$(cc -dumpmachine)
export HOST BUILD
# gnulib's stack-direction check knows no wasm32 and cannot run a test program
# when cross compiling, so it leaves STACK_DIRECTION 0. The stack (in linear
# memory) grows down. On Linux targets gnulib's sigsegv module needs the
# direction to find the stack's VMA.
export sv_cv_stack_direction=-1 sv_cv_stack_direction_msg="grows down"
# Autoconf's AC_FUNC_MALLOC/AC_FUNC_REALLOC can't run their test program when
# cross compiling and then ask for an rpl_malloc/rpl_realloc the package must
# provide (procps-ng, psmisc don't). libc2's (musl's) malloc(0) and
# realloc(p, 0) return unique pointers.
export ac_cv_func_malloc_0_nonnull=yes ac_cv_func_realloc_0_nonnull=yes

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
