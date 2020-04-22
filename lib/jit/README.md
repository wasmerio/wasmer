# Wasmer JIT

The Wasmer JIT is usable with any compiler implementation
based on `wasmer-compiler`.
After the compiler process the result, the JIT pushes it into
memory and links it's contents so it can be usable by the
`wasmer` api.

> Note: this project started as a subfork of [this crate](https://crates.io/crates/wasmtime-jit).
