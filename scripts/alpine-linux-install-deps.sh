#!/usr/bin/env sh
# Install package dependencies on Alpine linux.
#
# This script is used by the CI!

apk update
# `build-base` provides gcc, g++, libc-dev (musl-dev), binutils, and the
# /usr/bin/cc -> /usr/bin/gcc symlink that rustc invokes to link host-side
# build scripts (proc-macro2, quote, libc, serde_core, ...). Without it,
# `cargo build` fails with `error: linker 'cc' not found` on alpine:edge
# images that no longer ship a host C toolchain by default.
apk add build-base bash make curl cmake ninja clang22 zstd-static llvm22-dev clang22-static llvm22-static ncurses-static zlib-static tar libxml2-static xz-static

# A workaround for an unreleased clang-sys crate fix:
# https://github.com/rust-lang/rust-bindgen/issues/2360#issuecomment-2367084230
cat >/usr/bin/llvm-config <<'EOF'
#!/usr/bin/env bash

if [ "$1" = "--libs" ]; then
    echo `/usr/bin/llvm-config-22 "$@" "--link-static"` -lzstd
else
    /usr/bin/llvm-config-22 "$@"
fi
EOF
chmod +x /usr/bin/llvm-config
