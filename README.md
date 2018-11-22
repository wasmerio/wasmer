<p align="center"><a href="https://wasmer.io" target="_blank" rel="noopener noreferrer"><img width="400" src="https://raw.githubusercontent.com/WAFoundation/wasmer/master/logo.png" alt="Wasmer logo"></a></p>

<p align="center">
  <a href="https://circleci.com/gh/wafoundation/wasmer/"><img src="https://img.shields.io/circleci/project/github/WAFoundation/wasmer/master.svg" alt="Build Status"></a>
  <a href="https://github.com/WAFoundation/wasmer/blob/master/LICENSE"><img src="https://img.shields.io/github/license/WAFoundation/wasmer.svg" alt="License"></a>
</p>

## Introduction

Wasmer is a Standalone JIT-style WebAsssembly runtime code built on [Cranelift](https://github.com/CraneStation/cranelift) code generator engine.

## Usage

`wasmer` can execute both the standard binary format (`.wasm`) and the text
format defined by the WebAssembly reference interpreter (`.wat`).

Once installed, you will be able to run:

```sh
wasmer run my_wasm_file.wasm
```

## Building & Running

To build this project you will need Rust and Cargo.

```sh
# checkout code and associated submodules
git clone --recursive https://github.com/wafoundation/wasmer.git
cd wasmer

# install tools
# make sure that `python` is accessible.
cargo install
```

## Testing

Thanks to [spectests](https://github.com/WAFoundation/wasmer/tree/master/spectests) we can assure 100% compatibility with the WebAssembly spec test suite.

Tests can be run with:

```sh
make test
```

If you need to re-generate the Rust tests from the spectests
you can run:

```sh
make spectests
```

## Roadmap

Wasmer is an open project guided by strong principles, aiming to be modular, flexible and fast. It is open to the community to help set its direction.

Below are some of the goals (written with order) of this project:

- [x] It should be 100% compatible with the [WebAssembly Spectest](https://github.com/WAFoundation/wasmer/tree/master/spectests)
- [x] It should be fast _partially achieved_
- [ ] Support Emscripten calls _(on the works)_
- [ ] Support Rust ABI calls

## License

MIT/Apache-2.0
