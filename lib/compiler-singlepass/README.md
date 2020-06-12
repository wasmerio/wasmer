# Wasmer Compiler - Singlepass

This is the `wasmer-compiler-singlepass` crate, which contains a
compiler implementation based on Singlepass.

Singlepass is designed to emit compiled code at linear time, as such
is not prone to JIT bombs and also offers great compilation performance
orders of magnitude faster than `wasmer-compiler-cranelift` and
`wasmer-compiler-llvm`, however with a bit slower runtime speed.

The fact that singlepass is not prone to JIT bombs and offers a very
predictable compilation speed makes it ideal for **blockchains** and other
systems where fast and consistent compilation times are very critical.

## Requirements

At the moment, this crate depends on Rust nightly to be compiled, as it uses
`dynasm-rs` which can only be compiled in Nigthly.
