This is the `wasmer-runtime` crate, which contains wasm runtime library
support, supporting the wasm ABI used by [`wasmer-compiler-cranelift`],
[`wasmer-jit`], and [`wasmer-obj`].

This crate does not make a host vs. target distinction; it is meant to be
compiled for the target.

[`wasmer-compiler-cranelift`]: https://crates.io/crates/wasmer-compiler-cranelift
[`wasmer-jit`]: https://crates.io/crates/wasmer-jit
[`wasmer-obj`]: https://crates.io/crates/wasmer-obj
