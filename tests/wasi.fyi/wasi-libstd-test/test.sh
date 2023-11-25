#!/bin/bash
set -ueo pipefail

usage() {
  echo "Usage: $0 <runtime>"
  exit 1
}

if [ $# -ne 1 ]; then
  usage
else
  runtime=$1
fi

BASE_DIR=$(dirname "$0")

$BASE_DIR/build.sh

status=0

for input in $BASE_DIR/*.wasm; do
  echo "Testing $input..."
  $BASE_DIR/../tools/wasm-test $runtime $input || status=1
done

exit $status

