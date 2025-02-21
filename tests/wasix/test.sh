#!/bin/bash

if [[ -z "${WASI_SDK}" ]]; then
    echo "WASI_SDK is not found"
    exit 1
fi

if [[ -z "${WASIX_SYSROOT}" ]]; then
    echo "WASIX_SYSROOT is not found"
    exit 1
fi

export RANLIB="$WASI_SDK/bin/ranlib"
export AR="$WASI_SDK/bin/ar"
export NM="$WASI_SDK/bin/nm"
export CC="$WASI_SDK/bin/clang"
export CXX="$WASI_SDK/bin/clang"
export CFLAGS="\
--sysroot=$WASIX_SYSROOT \
-matomics \
-mbulk-memory \
-mmutable-globals \
-pthread \
-mthread-model posix \
-ftls-model=local-exec \
-fno-trapping-math \
-D_WASI_EMULATED_MMAN \
-D_WASI_EMULATED_SIGNAL \
-D_WASI_EMULATED_PROCESS_CLOCKS \
-O3 \
-g \
-flto"
export LD="$WASI_SDK/bin/wasm-ld"
export LDFLAGS="\
-Wl,--shared-memory \
-Wl,--max-memory=4294967296 \
-Wl,--import-memory \
-Wl,--export-dynamic \
-Wl,--export=__heap_base \
-Wl,--export=__stack_pointer \
-Wl,--export=__data_end \
-Wl,--export=__wasm_init_tls \
-Wl,--export=__wasm_signal \
-Wl,--export=__tls_size \
-Wl,--export=__tls_align \
-Wl,--export=__tls_base \
-lwasi-emulated-mman \
-O3 \
-g \
-flto"
export LIBS="\
-Wl,--shared-memory \
-Wl,--max-memory=4294967296 \
-Wl,--import-memory \
-Wl,--export-dynamic \
-Wl,--export=__heap_base \
-Wl,--export=__stack_pointer \
-Wl,--export=__data_end \
-Wl,--export=__wasm_init_tls \
-Wl,--export=__wasm_signal \
-Wl,--export=__tls_size \
-Wl,--export=__tls_align \
-Wl,--export=__tls_base \
-lwasi-emulated-mman \
-O3 \
-g \
-flto"

export WASMER=$(realpath "../../target/release/wasmer")

printf "\n\nStarting WASIX Test Suite:\n"

status=0
while read dir; do
    dir=$(basename "$dir")
    printf "Testing $dir..."

    cmd="cd $dir; \
        $CC $CFLAGS $LDFLAGS -o main-not-asyncified.wasm main.c; \
        wasm-opt --asyncify main-not-asyncified.wasm -o main.wasm; \
        ./run.sh"

    if bash -c "$cmd"; then
        printf "\rTesting $dir ✅\n"
    else
        printf "\rTesting $dir ❌\n"
        status=1
    fi
done < <(find . -mindepth 1 -maxdepth 1 -type d | sort)

exit $status