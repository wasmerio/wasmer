#!/usr/bin/env bash
# CI-only source producer for the unreleased WASIX v2 tools. No release builds.
# Runtime linking uses B2's separate pinned nightly recipe.
set -euo pipefail
cd "$(dirname "$0")/../../../../.."
source lib/wasixv2/api/tests/browser/toolchain.env
: "${WASIXV2_BROWSER_TOOLS:?set an isolated toolchain build/cache directory}"
tools=$(realpath -m "$WASIXV2_BROWSER_TOOLS")
mkdir -p "$tools"
pins=$(sha256sum lib/wasixv2/api/tests/browser/toolchain.env | cut -d' ' -f1)
[[ "$(git -C lib/wasixv2/codegen rev-parse HEAD)" == "$WASIX_BROWSER_ABI_REV" ]]
# Match the cross-repository GH_TOKEN pattern in .github/workflows/build.yml.
# Keep credentials out of source URLs and cached git configuration.
git_source() {
    if [[ -n "${GH_TOKEN:-}" ]]; then
        # shellcheck disable=SC2016 # Git expands GH_TOKEN only when authenticating.
        git -c credential.helper= \
            -c 'credential.helper=!f() { echo username=x-access-token; echo "password=$GH_TOKEN"; }; f' "$@"
    else
        git "$@"
    fi
}
checkout() {
    local repository=$1 revision=$2 directory=$3
    if [[ ! -d "$directory/.git" ]]; then
        git init "$directory"
        git -C "$directory" remote add origin "$repository"
    fi
    # A previous failed fetch may have left an initialized but empty checkout.
    if [[ "$(git -C "$directory" rev-parse HEAD 2>/dev/null || true)" != "$revision" ]]; then
        git_source -C "$directory" fetch --depth 1 origin "$revision"
        git -C "$directory" checkout --detach FETCH_HEAD
    fi
    [[ "$(git -C "$directory" rev-parse HEAD)" == "$revision" ]]
    [[ -z "$(git -C "$directory" status --porcelain --untracked-files=no)" ]]
}
if [[ ! -f "$tools/complete-$pins" ]]; then
    if ! git_source ls-remote https://github.com/wasix-org/libc2.git HEAD >/dev/null; then
        echo 'Private libc2 source is required: set WASIXV2_TOOLCHAIN_TOKEN with contents:read access to wasix-org/libc2.' >&2
        exit 1
    fi
    checkout https://github.com/wasix-org/llvm-project.git "$WASIX_BROWSER_LLVM_REV" "$tools/src/llvm"
    checkout https://github.com/wasix-org/wasixcc.git "$WASIX_BROWSER_WASIXCC_REV" "$tools/src/wasixcc"
    checkout https://github.com/wasix-org/libc2.git "$WASIX_BROWSER_LIBC2_REV" "$tools/src/libc2"
    checkout https://github.com/WebAssembly/binaryen.git "$WASIX_BROWSER_BINARYEN_REV" "$tools/src/binaryen"
    git_source -C "$tools/src/libc2" submodule update --init wasixv2/abi
    [[ "$(git -C "$tools/src/libc2/wasixv2/abi" rev-parse HEAD)" == "$WASIX_BROWSER_ABI_REV" ]]
    nice -n 19 taskset -c "${WASIX_BROWSER_CPUS:-4-7}" cmake -S "$tools/src/llvm/llvm" -B "$tools/build/llvm" -G Ninja \
        -DCMAKE_BUILD_TYPE=Debug -DCMAKE_C_FLAGS_DEBUG='-O0 -g0' -DCMAKE_CXX_FLAGS_DEBUG='-O0 -g0' \
        -DLLVM_ENABLE_ASSERTIONS=ON -DLLVM_PARALLEL_LINK_JOBS=1 -DLLVM_USE_LINKER=lld \
        -DLLVM_ENABLE_PROJECTS='clang;lld' -DLLVM_TARGETS_TO_BUILD=WebAssembly \
        -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF
    nice -n 19 taskset -c "${WASIX_BROWSER_CPUS:-4-7}" cmake --build "$tools/build/llvm" --parallel 3 --target clang lld llvm-ar llvm-ranlib llvm-nm
    ln -sf lld "$tools/build/llvm/bin/wasm-ld"
    nice -n 19 taskset -c "${WASIX_BROWSER_CPUS:-4-7}" cmake -S "$tools/src/binaryen" -B "$tools/build/binaryen" -G Ninja \
        -DCMAKE_BUILD_TYPE=Debug -DCMAKE_C_FLAGS_DEBUG='-O0 -g0' -DCMAKE_CXX_FLAGS_DEBUG='-O0 -g0' \
        -DBUILD_TESTS=OFF -DBUILD_STATIC_LIB=ON
    nice -n 19 taskset -c "${WASIX_BROWSER_CPUS:-4-7}" cmake --build "$tools/build/binaryen" --parallel 3 --target wasm-opt wasm-as
    rustup toolchain install "$WASIX_BROWSER_CC_TOOLCHAIN" --profile minimal
    CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$tools/build/wasixcc" nice -n 19 taskset -c "${WASIX_BROWSER_CPUS:-4-7}" \
        cargo +"$WASIX_BROWSER_CC_TOOLCHAIN" build --locked --manifest-path "$tools/src/wasixcc/Cargo.toml" --no-default-features -j3
    mkdir -p "$tools/bin"
    cp "$tools/build/wasixcc/debug/wasixccenv" "$tools/bin/"
    for name in wasixcc wasix++ wasixcc++ wasixar wasixnm wasixranlib wasixld; do ln -sf wasixccenv "$tools/bin/$name"; done
    for config in eh asyncify; do
        LLVM_BIN="$tools/build/llvm/bin" LLVM_SRC="$tools/src/llvm" \
            nice -n 19 taskset -c "${WASIX_BROWSER_CPUS:-4-7}" sh "$tools/src/libc2/wasixv2/build-sysroot.sh" \
            --out "$tools/sysroots" --target wasm32-wasixv2 --config "$config" --no-cxx -j3
    done
fi
# A cache marker alone must not bless an incomplete/corrupt installation.
for executable in bin/wasixccenv build/llvm/bin/clang build/llvm/bin/wasm-ld \
    build/llvm/bin/llvm-ar build/llvm/bin/llvm-ranlib build/llvm/bin/llvm-nm build/binaryen/bin/wasm-opt; do
    [[ -x "$tools/$executable" ]] || { echo "Missing tool: $tools/$executable" >&2; exit 1; }
done
compgen -G "$tools/build/llvm/lib/clang/*/include/stddef.h" >/dev/null || {
    echo 'Missing Clang resource headers' >&2
    exit 1
}
for sysroot in sysroot-wasixv2 sysroot-wasixv2-asyncify; do
    for file in include/stdio.h lib/wasm32-wasixv2/libc.a lib/wasm32-wasixv2/crt1.o lib/wasm32-wasixv2/libclang_rt.builtins.a; do
        [[ -f "$tools/sysroots/$sysroot/$file" ]] || { echo "Missing sysroot file: $sysroot/$file" >&2; exit 1; }
    done
done
touch "$tools/complete-$pins"
# The workflow sources this file after cache restore as well as after a cold build.
python3 - "$tools" <<'PY'
import pathlib, shlex, sys
root = pathlib.Path(sys.argv[1])
values = {
    'PATH': str(root/'bin') + ':' + str(root/'build/binaryen/bin'),
    'WASIXCC_LLVM_LOCATION': str(root/'build/llvm'),
    'WASIXCC_SYSROOT_PREFIX': str(root/'sysroots'),
    'WASIXCC_BINARYEN_LOCATION': str(root/'build/binaryen'),
}
lines = ['export PATH=' + shlex.quote(values.pop('PATH')) + ':"$PATH"']
lines += ['export ' + key + '=' + shlex.quote(value) for key, value in values.items()]
(root/'env.sh').write_text('\n'.join(lines) + '\n')
PY
