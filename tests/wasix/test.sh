#!/bin/bash

export WASMER="$(realpath "../../target/release/wasmer")"
status=0
run_tests() {
    local backend="$1"
    export WASMER_RUN="${WASMER} run -q ${backend}"

    printf "\n\nStarting WASIX Test Suite ($backend):\n"
    while read dir; do
        dir=$(basename "$dir")
        printf "Testing $backend: $dir...\r"
        if (
            cd "$dir" || exit 1
            find . -name 'output*' | xargs rm -f

            if [ ! -e .no-build ]; then
                local extra_flags=""
                if [ -f .flags ]; then
                    extra_flags="$(< .flags)"
                fi

                find . -name '*.wasm' | xargs rm -f
                if [ -f main.cc ]; then
                    wasix++ main.cc -o main.wasm ${extra_flags}
                else
                    wasixcc -sWASM_EXCEPTIONS=false main.c -o main.wasm ${extra_flags}
                fi
            fi

            ./run.sh
        ); then
            printf "Testing $backend: $dir ✅\n"
        else
            printf "Testing $backend: $dir ❌\n"
            status=1
        fi
    done < <(find . -mindepth 1 -maxdepth 1 -type d | sort)
}

# Call the function with the desired backend argument
run_tests "--llvm"
run_tests "--cranelift"

exit $status
