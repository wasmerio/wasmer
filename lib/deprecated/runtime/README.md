# `wasmer-runtime` [DEPRECATED] [![Build Status](https://github.com/wasmerio/wasmer-reborn/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer-reborn/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

## Deprecation notice: please read

Thanks to users feedback, collected experience and various use cases,
Wasmer has decided to entirely improve its API to offer the best user
experience and the best features to as many users as possible.

The new version of Wasmer (`1.0.0-alpha.1`) includes many improvements
in terms of performance or the memory consumption, in addition to a ton
of new features and much better flexibility!
You can check revamped new API in the [`wasmer`] crate.

In order to help our existing users to enjoy the performance boost and
memory improvements without updating their program that much, we have
created a new version of the `wasmer-runtime` crate, which is now
*an adaptation* of the new API but with the old API syntax, as much as
possible. Indeed, it was not always possible to provide the exact same
API, but changes are subtle.

We have carefully documented most of the differences in [the
`runtime-core/CHANGES.md` document](../runtme-core/CHANGES.md).

It is important to understand the public of this port. We do not
recommend to advanced users of Wasmer to use this port. Advanced API,
like `ModuleInfo` or the `vm` module (incl. `vm::Ctx`) have not been
fully ported because it was very internals to Wasmer. For advanced
users, we highly recommend to migrate to the new version of Wasmer,
which is awesome by the way (completely neutral opinion). The public
for this port is beginners or regular users that do not necesarily
have time to update their code immediately but that want to enjoy a
performance boost and memory improvements.

[`wasmer`]: https://crates.io/crates/wasmer-runtime/

## Introduction

Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
compatible with Emscripten, Rust and Go. [Learn
more](https://github.com/wasmerio/wasmer).

This crate represents the high-level runtime API, making embedding
WebAssembly in your application easy, efficient, and safe.

### How to use Wasmer Runtime

The easiest way is to use the [`instantiate`] function to create an
[`Instance`]. Then you can use [`call`] or [`func`] and then
[`call`][func.call] to call an exported function safely.

[`instantiate`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/fn.instantiate.html
[`Instance`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Instance.html
[`call`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Instance.html#method.call
[`func`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Instance.html#method.func
[func.call]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/struct.Function.html#method.call

### Example

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
use wasmer_runtime::{imports, instantiate, DynFunc, Value};

static WASM: &'static [u8] = &[
    // The module above compiled to bytecode goes here.
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60,
    0x01, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07,
    0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x00, 0x00, 0x0a, 0x09, 0x01,
    0x07, 0x00, 0x20, 0x00, 0x41, 0x01, 0x6a, 0x0b, 0x00, 0x1a, 0x04, 0x6e,
    0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07, 0x61, 0x64, 0x64, 0x5f,
    0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02, 0x70, 0x30,
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // We're not importing anything, so make an empty import object.
    let import_object = imports! {};

    // Compile _and_ instantiate the Wasm module.
    let instance = instantiate(WASM, &import_object)?;

    // Call the `add_one` exported function.
    let values = instance
        .exports
        .get::<DynFunc>("add_one")?
        .call(&[Value::I32(42)])?;

    assert_eq!(values[0], Value::I32(43));
    
    Ok(())
}
```

### Additional Notes

The `wasmer-runtime` crate is built to support multiple compiler
backends:

* [`wasmer-compiler-singlepass`],
* [`wasmer-compiler-cranelift`],
* [`wasmer-compiler-llvm`].

You can specify the compiler you wish to use with the [`compile_with`] function:

```rust
use wasmer_runtime::{compile_with, Backend};

let module = compile_with(wasm_bytes, Backend::LLVM)?;
```

[`compile_with`]: https://docs.rs/wasmer-runtime/*/wasmer_runtime/fn.compile_with.html
[`wasmer-compiler-singlepass`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-singlepass
[`wasmer-compiler-cranelift`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-cranelift
[`wasmer-compiler-llvm`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-llvm
