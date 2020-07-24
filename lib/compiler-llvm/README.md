# `wasmer-compiler-llvm` [![Build Status](https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square)](https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

This crate contains a compiler implementation based on [the LLVM Compiler Infrastructure][LLVM].

## Usage

First, add this crate into your `Cargo.toml` dependencies:

```toml
wasmer-compiler-llvm = "1.0.0-alpha.1"
```

And then:

```rust
use wasmer::{Store, JIT};
use wasmer_compiler_llvm::LLVM;

let compiler = LLVM::new();
// Put it into an engine and add it to the store
let store = Store::new(&JIT::new(&compiler).engine());
```

*Note: you can find a [full working example using LLVM compiler here][example].*

## When to use LLVM

We recommend using LLVM as the default compiler when running WebAssembly
files on any **production** system, as it offers maximum peformance near
to native speeds.

## Requirements

The LLVM compiler requires a valid installation of LLVM in your system.
It currently requires **LLVM 10**.


You can install LLVM easily on your Debian-like system via this command:

```bash
bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)"
```

Or in macOS:

```bash
brew install llvm
```

Or via any of the [pre-built binaries that LLVM offers][llvm-pre-built].


[LLVM]: https://llvm.org/
[example]: https://github.com/wasmerio/wasmer-reborn/blob/master/examples/compiler_llvm.rs
[llvm-pre-built]: https://releases.llvm.org/download.html
