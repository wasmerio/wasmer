# Wasmer Compiler - Singlepass

This is the `wasmer-compiler-singlepass` crate, which contains a
compiler implementation based on Singlepass.

Singlepass is designed to emit compiled code at linear time, as such
is not prone to JIT bombs and also offers great compilation performance
orders of magnitude faster than `wasmer-compiler-cranelift` and
`wasmer-compiler-llvm`, however with a bit slower runtime speed.

> Note: this crate requires on Rust nightly to be compiled, as depends on
`dynasm-rs` and that crate can only be compiled in Nigthly.
