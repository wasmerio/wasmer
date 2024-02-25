#!/usr/bin/env bash

set -e

DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# get absolute path of the root dir
ROOT_DIR="$( cd "${DIR}/../../" && pwd )"
TEST_RUNNER="${TEST_RUNNER:-$ROOT_DIR/target/release/wasmer}"

if [ ! -e "$TEST_RUNNER" ]; then
  echo "Test runner not found at '$TEST_RUNNER' - set TEST_RUNNER env var to the correct path."
  exit 1
fi

input=$1

input_dir=$(dirname $input)
cd $input_dir

input_base=$(basename $input .wasm)

if [ -e "$input_base.stdin" ]; then
  stdin="$input_base.stdin"
else
  stdin="/dev/null"
fi

out_dir="$(mktemp -d)"
stdout_actual="$out_dir/stdout"
stderr_actual="$out_dir/stderr"
status_actual="$out_dir/status"

if [ -e "$input_base.arg" ]; then
  arg=$(cat "$input_base.arg")
else
  arg=""
fi

if [ -e "$input_base.dir" ]; then
  dir="--dir $input_base.dir"
else
  dir=""
fi

if [ -e "$input_base.env" ]; then
  env=$(sed -e 's/^/--env /' < "$input_base.env")
else
  env=""
fi

status=0

$TEST_RUNNER --mapdir /hamlet:./test_fs/hamlet --mapdir /fyi:./test_fs/fyi "$input_base.wasm" $dir $env -- $arg \
    < "$stdin" \
    > "$stdout_actual" \
    2> "$stderr_actual" \
    || status=$?

echo $status > "$status_actual"

stdout_expected="$input_base.stdout"
if [ -e "$stdout_expected" ]; then
  diff -u "$stdout_expected" "$stdout_actual"
fi

stderr_expected="$input_base.stderr"
if [ -e "$stderr_expected" ]; then
  diff -u "$stderr_expected" "$stderr_actual"
fi

status_expected="$input_base.status"
if [ -e "$input_base.status" ]; then
  diff -u "$status_expected" "$status_actual"
elif [ ! "$status" -eq "0" ]; then
  cat $stderr_actual
  exit 1
fi
