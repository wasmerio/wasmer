# `wasmer-compiler-llvm` [![Build Status](https://github.com/wasmerio/wasmer/actions/workflows/build.yml/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/main/LICENSE) [![crates.io](https://img.shields.io/crates/v/wasmer-compiler-llvm.svg)](https://crates.io/crates/wasmer-compiler-llvm)

This crate contains a compiler implementation based on [the LLVM Compiler Infrastructure][LLVM].

## Usage

```rust
use wasmer::{Store, EngineBuilder};
use wasmer_compiler_llvm::LLVM;

let compiler = LLVM::new();
let mut store = Store::new(compiler);
```

*Note: you can find a [full working example using LLVM compiler here][example].*

## When to use LLVM

We recommend using LLVM as the default compiler when running WebAssembly
files on any **production** system, as it offers maximum performance near
to native speeds.

## Requirements

The LLVM compiler requires a valid installation of LLVM in your system.
It currently requires **LLVM 18**.


You can install LLVM easily on your Debian-like system via this command:

```bash
wget https://apt.llvm.org/llvm.sh -O /tmp/llvm.sh
sudo bash /tmp/llvm.sh 18
```

Or in macOS:

```bash
brew install llvm@18
```

Or via any of the [pre-built binaries that LLVM offers][llvm-pre-built].


[LLVM]: https://llvm.org/
[example]: https://github.com/wasmerio/wasmer/blob/main/examples/compiler_llvm.rs
[llvm-pre-built]: https://releases.llvm.org/download.html
