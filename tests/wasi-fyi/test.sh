#!/usr/bin/env bash
set -ueo pipefail

bash build.sh

status=0

# Define skip list as an array
SKIP_LIST=("fs_create_dir-existing-directory.wasm")

# List and process .foo files
for file in *.wasm; do
    if [[ " ${SKIP_LIST[@]} " =~ " ${file} " ]]; then
        echo "Skipping $file"
    else
        echo "Testing $file"
        ./wasm-test.sh $file || status=1
    fi
done

exit $status

