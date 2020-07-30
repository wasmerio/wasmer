# `wasmer-compiler-singlepass` [![Build Status](https://github.com/wasmerio/wasmer-reborn/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer-reborn/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

This crate contains a compiler implementation based on the Singlepass linear compiler.

## Usage

Add this crate into your `Cargo.toml` dependencies:

```toml
wasmer-compiler-singlepass = "1.0.0-alpha.1"
```

And then:

```rust
use wasmer::{Store, JIT};
use wasmer_compiler_singlepass::Singlepass;

let compiler = Singlepass::new();
// Put it into an engine and add it to the store
let store = Store::new(&JIT::new(&compiler).engine());
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


[example]: https://github.com/wasmerio/wasmer-reborn/blob/master/examples/compiler_singlepass.rs
[`wasmer-compiler-cranelift`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-cranelift
[`wasmer-compiler-llvm`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-llvm
