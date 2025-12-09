#!/bin/bash

export WASMER=$(realpath "../../target/release/wasmer")

printf "\n\nStarting WASIX Test Suite:\n"

status=0
while read dir; do
    dir=$(basename "$dir")
    printf "Testing $dir...\r"

    if [ -e "$dir/.no-build" ]; then
        cmd="cd $dir; \
            find . -name 'output*' | xargs rm -f; \
            ./run.sh"
    else
        cmd="cd $dir; \
            find . -name 'output*' | xargs rm -f; \
            find . -name '*.wasm' | xargs rm -f; \
            wasixcc main.c -o main.wasm; \
            ./run.sh"
    fi

    if bash -c "$cmd"; then
        printf "Testing $dir ✅\n"
    else
        printf "Testing $dir ❌\n"
        status=1
    fi
done < <(find . -mindepth 1 -maxdepth 1 -type d | sort)

exit $status