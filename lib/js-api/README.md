# `wasmer-js` [![Build Status](https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE) [![crates.io](https://img.shields.io/crates/v/wasmer-js.svg)](https://crates.io/crates/wasmer-js)

[`Wasmer`](https://wasmer.io/) is the most popular
[WebAssembly](https://webassembly.org/) runtime for Rust. This crate mimics the same Rust
API than the `wasmer` crate, but when compiled to WebAssembly, it only targets
a JavaScript host. It means that it is possible to write a Rust program that uses Wasmer,
and compiles everything to WebAssembly to run in a browser, Node.js, Deno and so on.

This crate doesn't ship with any compilers or engines, as it leverages the Javascript VM to
compile and run WebAssembly.

## Usage

We recommend aliasing `wasmer_js` to `wasmer` at the top of your crate.

```rust
#[cfg(feature = "js")]
extern crate wasmer_js as wasmer;
```

And then:

```rust
use wasmer::{Store, Module, Instance, Value, imports};

#[wasm_bindgen]
pub extern fn do_add_one_in_wasmer() -> i32 {
    let module_wat = r#"
    (module
    (type $t0 (func (param i32) (result i32)))
    (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
        get_local $p0
        i32.const 1
        i32.add))
    "#;

    let store = Store::default();
    let module = Module::new(&store, &module_wat).unwrap();
    // The module doesn't import anything, so we create an empty import object.
    let import_object = imports! {};
    let instance = Instance::new(&module, &import_object).unwrap();

    let add_one = instance.exports.get_function("add_one").unwrap();
    let result = add_one.call(&[Value::I32(42)]).unwrap();
    assert_eq!(result[0], Value::I32(43));
    result[0].unwrap_i32()
}
```

## Feature flags

`wasmer-js` has the following feature flags:
* `wasm-types-polyfill` (enabled by default): it parses the Wasm file, allowing to do type reflection of the inner WebAssembly types.
  __It adds 100Kb to the Wasm bundle__ (28Kb gzipped). You can disable it and use `Module::set_type_hints` manually instead if you want a lightweight alternative.
  This is needed until the [Wasm JS introspection API proposal](https://github.com/WebAssembly/js-types/blob/master/proposals/js-types/Overview.md) is adopted by browsers

* `wat`: It allows to read WebAssembly files in their text format.
  *This feature is normally used only in development environments, __it will add around 650Kb to the Wasm bundle__* (120Kb gzipped).

# Build

You can use [`wasm-pack`](https://github.com/rustwasm/wasm-pack/) to build `wasmer-js-api`:

```
wasm-pack build --release
```

> The provided `wasmer_js.wasm` file should weight around 60kB (27Kb gzipped) when optmized via `wasm-opt` and stripped via `wasm-strip`, so it's quite slim.

# Test

```
wasm-pack test --node
```

---

Made with ❤️ by the Wasmer team, for the community
