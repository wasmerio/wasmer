#!/bin/bash

export WASMER=$(realpath "../../target/release/wasmer")

printf "\n\nStarting WASIX Test Suite:\n"

status=0
while read dir; do
    dir=$(basename "$dir")
    printf "Testing $dir..."

    if [ -e "$dir/.no-build" ]; then
        cmd="cd $dir; \
            ./run.sh"
    else
        cmd="cd $dir; \
            wasixcc main.c -o main.wasm; \
            ./run.sh"
    fi

    if bash -c "$cmd"; then
        printf "\rTesting $dir ✅\n"
    else
        printf "\rTesting $dir ❌\n"
        status=1
    fi
done < <(find . -mindepth 1 -maxdepth 1 -type d | sort)

exit $status