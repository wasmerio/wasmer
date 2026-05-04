#!/bin/bash

$WASMER_RUN main.wasm --volume=.:/data > output

status=0
printf "0" | diff -u output - 1>/dev/null

status=$?

rm -f my_file.txt

exit $status
