# `wasmer-cli` [![Build Status](https://github.com/wasmerio/wasmer/actions/workflows/build.yml/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/main/LICENSE)

This crate is the Wasmer CLI.

The recommended way to install `wasmer` is via the [wasmer-installer](https://github.com/wasmerio/wasmer-install).

However, you can also install `wasmer` via Cargo (you will need to specify the compilers to use):

```bash
cargo install wasmer-cli --features "singlepass,cranelift"
```

Or by building it inside the codebase:

```bash
cargo build --release --features "singlepass,cranelift"
```

## Features

The Wasmer supports the following features:
* `wat` (default): support for executing WebAssembly text files.
* `wast`(default): support for running wast test files.
* `cache` (default): support or automatically caching compiled artifacts.
* `wasi` (default): support for [WASI].
* `singlepass`: support for the [Singlepass compiler].
* `cranelift`: support for the [Cranelift compiler].
* `llvm`: support for the [LLVM compiler].

[WASI]: https://github.com/wasmerio/wasmer/tree/main/lib/wasi/
[Singlepass compiler]: https://github.com/wasmerio/wasmer/tree/main/lib/compiler-singlepass/
[Cranelift compiler]: https://github.com/wasmerio/wasmer/tree/main/lib/compiler-cranelift/
[LLVM compiler]: https://github.com/wasmerio/wasmer/tree/main/lib/compiler-llvm/

## CLI commands

Once you have Wasmer installed, you can start executing WebAssembly files easily:

Get the current Wasmer version:

```bash
wasmer -V
```

Execute a WebAssembly file:

```bash
wasmer run myfile.wasm
```

Compile a WebAssembly file:

```bash
wasmer compile myfile.wasm -o myfile.wasmu
```

Run a compiled WebAssembly file (fastest):

```bash
wasmer run myfile.wasmu
```
