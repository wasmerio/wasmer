#!/bin/bash

set -eu

rm -f output*

RUST_LOG=virtual_fs=trace $WASMER run main.wasm --dir . &> output-wasmer-log

cat output | grep "parent 1" >/dev/null && \
  cat output | grep "parent 2" >/dev/null && \
  cat output | grep "child 1" >/dev/null && \
  cat output | grep "child 2" >/dev/null && \
  awk '/closing last fd/{m1=1} /Closing host file.*shared-fd\/output/{if(m1) m2=1} /last fd closed/{if(m2) print "found all lines"}' output-wasmer-log | grep "found all lines" >/dev/null
