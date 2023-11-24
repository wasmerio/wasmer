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

bash build.sh

status=0

for input in *.wasm; do
  echo "Testing $input..."
  tools/wasm-test $runtime $input || status=1
done

exit $status

