<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="400" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://circleci.com/gh/wasmerio/wasmer/">
    <img src="https://img.shields.io/circleci/project/github/wasmerio/wasmer/master.svg" alt="Build Status">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
  <a href="https://crates.io/crates/wasmer-runtime">
    <img src="https://img.shields.io/crates/d/wasmer-runtime.svg" alt="Number of downloads from crates.io">
  </a>
  <a href="https://docs.rs/wasmer-runtime">
    <img src="https://docs.rs/wasmer-runtime/badge.svg" alt="Read our API documentation">
  </a>
</p>

# Wasmer Runtime

Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
compatible with Emscripten, Rust and Go. [Learn
more](https://github.com/wasmerio/wasmer).

This crate represents the high-level runtime API, making embedding
WebAssembly in your application easy, efficient, and safe.

## How to use Wasmer Runtime

The easiest way is to use the [`instantiate`] function to create an
[`Instance`]. Then you can use [`call`] or [`func`] and then
[`call`][func.call] to call an exported function safely.

[`instantiate`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/fn.instantiate.html
[`Instance`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Instance.html
[`call`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Instance.html#method.call
[`func`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Instance.html#method.func
[func.call]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Function.html#method.call

## Example

Given this WebAssembly:

```wat
(module
  (type $t0 (func (param i32) (result i32)))
  (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
    get_local $p0
    i32.const 1
    i32.add))
```

compiled into Wasm bytecode, we can call the exported `add_one` function:

```rust
static WASM: &'static [u8] = &[
    // The module above compiled to bytecode goes here.
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60,
    0x01, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07,
    0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x00, 0x00, 0x0a, 0x09, 0x01,
    0x07, 0x00, 0x20, 0x00, 0x41, 0x01, 0x6a, 0x0b, 0x00, 0x1a, 0x04, 0x6e,
    0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07, 0x61, 0x64, 0x64, 0x5f,
    0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02, 0x70, 0x30,
];

use wasmer_runtime::{
    instantiate,
    Value,
    imports,
    error,
};

fn main() -> error::Result<()> {
    // We're not importing anything, so make an empty import object.
    let import_object = imports! {};

    let mut instance = instantiate(WASM, &import_object)?;

    let values = instance
        .func("add_one")?
        .call(&[Value::I32(42)])?;

    assert_eq!(values[0], Value::I32(43));
    
    Ok(())
}
```

## Additional Notes

The `wasmer-runtime` crate is build to support multiple compiler
backends.  Currently, we support the [Cranelift] compiler with the
[`wasmer-clif-backend`] crate by default.

You can specify the compiler you wish to use with the [`compile_with`] function.

[Cranelift]: https://github.com/CraneStation/cranelift
[`wasmer-clif-backend`]: https://crates.io/crates/wasmer-clif-backend
[`compile_with`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/fn.compile_with.html
