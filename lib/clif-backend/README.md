<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="400" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://circleci.com/gh/wasmerio/wasmer/">
    <img src="https://img.shields.io/circleci/project/github/wasmerio/wasmer/master.svg" alt="Build Status">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
  <a href="https://crates.io/crates/wasmer-clif-backend">
    <img src="https://img.shields.io/crates/d/wasmer-clif-backend.svg" alt="Number of downloads from crates.io">
  </a>
  <a href="https://docs.rs/wasmer-clif-backend">
    <img src="https://docs.rs/wasmer-clif-backend/badge.svg" alt="Read our API documentation">
  </a>
</p>

# Wasmer Cranelift backend

Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
compatible with Emscripten, Rust and Go. [Learn
more](https://github.com/wasmerio/wasmer).

This crate represents the Cranelift backend integration for Wasmer.

## Usage

### Usage in Wasmer Standalone

If you are using the `wasmer` CLI, you can specify the backend with:

```bash
wasmer run program.wasm --backend=cranelift
```

### Usage in Wasmer Embedded

If you are using Wasmer Embedded, you can specify
the Cranelift backend to the [`compile_with` function](https://docs.rs/wasmer-runtime-core/*/wasmer_runtime_core/fn.compile_with.html):

```rust
use wasmer_clif_backend::CraneliftCompiler;

// ...
let module = wasmer_runtime_core::compile_with(&wasm_binary[..], &CraneliftCompiler::new());
```
