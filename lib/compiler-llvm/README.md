<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-compiler-llvm</code> library</h1>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="Build Status" />
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License" />
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Slack channel" />
    </a>
    <a href="https://crates.io/crates/wasmer-compiler-llvm">
      <img src="https://img.shields.io/crates/v/wasmer-compiler-llvm.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_compiler_llvm/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

This crate contains a compiler implementation based on [the LLVM Compiler Infrastructure][LLVM].

## Usage

```rust
use wasmer::{Store, Universal};
use wasmer_compiler_llvm::LLVM;

let compiler = LLVM::new();
// Put it into an engine and add it to the store
let store = Store::new(&Universal::new(compiler).engine());
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
[example]: https://github.com/wasmerio/wasmer/blob/master/examples/compiler_llvm.rs
[llvm-pre-built]: https://releases.llvm.org/download.html
