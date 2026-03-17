#!/bin/bash

set -e

TEMP_DIR1=$(mktemp -d)
TEMP_DIR2=$(mktemp -d)

trap 'rm -rf "$TEMP_DIR1" "$TEMP_DIR2" output 2>/dev/null' EXIT

printf "left" > "$TEMP_DIR1/target.txt"
printf "right" > "$TEMP_DIR2/target.txt"
ln -s target.txt "$TEMP_DIR1/link.txt"
ln -s target.txt "$TEMP_DIR2/link.txt"

$WASMER_RUN main.wasm --volume "$TEMP_DIR1:/a" --volume "$TEMP_DIR2:/b" > output

printf "0" | diff -u output - 1>/dev/null
