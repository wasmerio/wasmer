<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-compiler-cranelift</code> library</h1>

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
    <a href="https://crates.io/crates/wasmer-compiler-cranelift">
      <img src="https://img.shields.io/crates/v/wasmer-compiler-cranelift.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_compiler_cranelift/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

This crate contains a compiler implementation based on Cranelift.

## Usage

```rust
use wasmer::{Store, Universal};
use wasmer_compiler_cranelift::Cranelift;

let compiler = Cranelift::new();
// Put it into an engine and add it to the store
let store = Store::new(&Universal::new(compiler).engine());
```

*Note: you can find a [full working example using Cranelift compiler
here][example].*

## When to use Cranelift

We recommend using this compiler crate **only for development
proposes**. For production we recommend using [`wasmer-compiler-llvm`]
as it offers a much better runtime speed (50% faster on average).

### Acknowledgments

This project borrowed some of the function lowering from
[`cranelift-wasm`].

Please check [Wasmer `ATTRIBUTIONS`] to further see licenses and other
attributions of the project.


[example]: https://github.com/wasmerio/wasmer/blob/master/examples/compiler_cranelift.rs
[`wasmer-compiler-llvm`]: https://github.com/wasmerio/wasmer/tree/master/lib/compiler-llvm
[`cranelift-wasm`]: https://crates.io/crates/cranelift-wasm
[Wasmer `ATTRIBUTIONS`]: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md
