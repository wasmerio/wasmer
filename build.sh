#!/bin/sh

cargo build --target=riscv64gc-unknown-linux-gnu  --example riscv --no-default-features --features "singlepass"
