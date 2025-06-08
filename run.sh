#!/bin/sh

RUST_BACKTRACE=1
LD_LIBRARY_PATH=/usr/riscv64-linux-gnu/lib/

export RUST_BACKTRACE LD_LIBRARY_PATH

qemu-riscv64-static -g 1024 target/riscv64gc-unknown-linux-gnu/debug/examples/riscv &

(echo set debuginfod enabled on
 echo target remote localhost:1024
 echo br rust_begin_unwind
 echo br core::panicking::panic
 cat -u /dev/tty) |
rust-gdb target/riscv64gc-unknown-linux-gnu/debug/examples/riscv
