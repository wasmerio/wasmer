# `wasmer-compiler-singlepass` 
[![Build Status](https://github.com/wasmerio/wasmer/actions/workflows/build.yml/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/main/LICENSE) [![crates.io](https://img.shields.io/crates/v/wasmer-compiler-singlepass.svg)](https://crates.io/crates/wasmer-compiler-singlepass)

This crate contains a compiler implementation based on the Singlepass linear compiler.
Supported targets:
- x86_64
- aarch64
- riscv32 (experimental)
- riscv64 (experimental)
## Usage

```rust
use wasmer::{Store, sys::EngineBuilder};
use wasmer_compiler_singlepass::Singlepass;

let compiler = Singlepass::new();
let mut store = Store::new(compiler);
```

*Note: you can find a [full working example using Singlepass compiler
here][example].*

## When to use Singlepass

Singlepass is designed to emit compiled code at linear time, as such
is not prone to JIT bombs and also offers great compilation
performance orders of magnitude faster than
[`wasmer-compiler-cranelift`] and [`wasmer-compiler-llvm`], however
with a bit slower runtime speed.

The fact that singlepass is not prone to JIT bombs and offers a very
predictable compilation speed makes it ideal for **blockchains** and other
systems where fast and consistent compilation times are very critical.


[example]: https://github.com/wasmerio/wasmer/blob/main/examples/compiler_singlepass.rs
[`wasmer-compiler-cranelift`]: https://github.com/wasmerio/wasmer/tree/main/lib/compiler-cranelift
[`wasmer-compiler-llvm`]: https://github.com/wasmerio/wasmer/tree/main/lib/compiler-llvm
