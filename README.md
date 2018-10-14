# wasmer - WebAssembly runtime

[![Build Status](https://api.travis-ci.org/wapmio/wasmer.svg?branch=master)](https://travis-ci.org/rust-lang-nursery/error-chain)
[![Latest Version](https://img.shields.io/crates/v/wasmer.svg)](https://crates.io/crates/error-chain)
[![License](https://img.shields.io/github/license/wapmio/wasmer.svg)](https://github.com/wapmio/wasmer)

`wasmer` is a Standalone JIT-style runtime for WebAsssembly code.

The [Cranelift](https://github.com/CraneStation/cranelift) compiler is used to compile WebAssembly to native machine code. Once compiled, there are no complex interactions between the application and the runtime (unlike jit compilers, like v8) to reduce surface area for vulnerabilities.

**THIS PROJECT IS NOT USABLE YET, BUT WILL BE SOON 🙂**

[Documentation (crates.io)](https://docs.rs/wasmer).

## Principles

Wasmer is an open project guided by strong principles, aiming to be modular, flexible and fast. It is open to the community to help set its direction.

- Modular: the project includes lots of components that have well-defined functions and APIs that work together.
- Tested: All WebAssembly spec test cases should be covered.
- Developer focused: The APIs are intended to be functional and useful to build powerful tools.
- Fast: it should be as fast as possible.

## Usage

It can load both the standard binary format (`.wasm`), and the text format
defined by the WebAssembly reference interpreter (`.wat`).

Once installed, you will be able to run:

```sh
wasmer run my_wasm_file.wasm
```

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

## Testing

Tests can be run with:

```sh
cargo test
```

## License

MIT/Apache-2.0
