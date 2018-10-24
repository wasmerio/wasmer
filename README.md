# wasmer - WebAssembly Runtime

[![Build Status](https://api.travis-ci.com/WAFoundation/wasmer.svg?branch=master)](https://travis-ci.com/WAFoundation/wasmer)
[![Latest Version](https://img.shields.io/crates/v/wasmer.svg)](https://crates.io/crates/wasmer)
[![License](https://img.shields.io/github/license/WAFoundation/wasmer.svg)](https://github.com/WAFoundation/wasmer)

`wasmer` is a Standalone JIT-style runtime for WebAsssembly code.

The [Cranelift](https://github.com/CraneStation/cranelift) compiler is used to compile WebAssembly to native machine code. Once compiled, there are no complex interactions between the application and the runtime (unlike jit compilers, like v8) to reduce surface area for vulnerabilities.

[Documentation (crates.io)](https://docs.rs/wasmer).

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
git clone https://github.com/wafoundation/wasmer.git
cd wasmer

# install tools
# make sure that `python` is accessible.
cargo install
```

## Testing

This library should be always fully tested.

Thanks to [spectests](spectests/) we can assure 100% compatibility with the WebAssembly spec test suite.

Tests can be run with:

```sh
cargo test
```

If you need to re-generate the Rust tests from the spectests
you can run:

```sh
make spectests
```

## Roadmap

Wasmer is an open project guided by strong principles, aiming to be modular, flexible and fast. It is open to the community to help set its direction.

Below are some of the goals (written with order) of this project:

- [ ] It should be 100% compatible with the WebAssembly Spectest (on the works)
- [ ] It should be fast. We can achieve this by caching the function compilations
- [ ] Support Emscripten calls
- [ ] Support Rust ABI calls

## License

MIT/Apache-2.0
