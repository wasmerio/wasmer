# wasmer - WebAssembly runtime

[![Build Status](https://api.travis-ci.org/wapmio/wasmer.svg?branch=master)](https://travis-ci.org/rust-lang-nursery/error-chain)
[![Latest Version](https://img.shields.io/crates/v/wasmer.svg)](https://crates.io/crates/error-chain)
[![License](https://img.shields.io/github/license/wapmio/wasmer.svg)](https://github.com/wapmio/wasmer)

`wasmer` is a Standalone JIT-style runtime for WebAsssembly code.

The [Cranelift](https://github.com/CraneStation/cranelift) compiler is used to compile WebAssembly to native machine code. Once compiled, there are no complex interactions between the application and the runtime (unlike jit compilers, like v8) to reduce surface area for vulnerabilities.

[Documentation (crates.io)](https://docs.rs/wasmer).

## Usage

It can load both the standard binary format (`.wasm`), and the text format
defined by the WebAssembly reference interpreter (`.wat`).

## Building & Running

To build this project you will need Rust and Cargo.

```sh
# checkout code and associated submodules
git clone https://github.com/wapmio/wasmer.git
cd wasmer

# install tools
# make sure that `python` is accessible.
cargo install
```

## License

MIT/Apache-2.0
