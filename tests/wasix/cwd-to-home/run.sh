#!/bin/bash

$WASMER -q run main.wasm --dir=. > output

diff -u output expected 1>/dev/null