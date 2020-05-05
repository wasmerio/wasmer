# Wasmer [![Build Status](https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square)](https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

[`Wasmer`](https://wasmer.io/) is the most popular [WebAssembly](https://webassembly.org/)
runtime for Rust. It supports JIT (Just in Time) and AOT (Ahead of time)
compilation as well as mulitple compiler implementations.

It's designed to be safe and secure, and runnable in any kind of environment.

## Usage

Add to your `Cargo.toml`

```
[dependencies]
wasmer = "0.16.2"
```

```rust
use wasmer::{Instance, Function, Value, imports, DefaultStore as _};

fn main() -> error::Result<()> {
    let module_wat = r#"
    (module
    (type $t0 (func (param i32) (result i32)))
    (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
        get_local $p0
        i32.const 1
        i32.add))
    "#;

    let module = Module::new(&module_wat);
    // We're not importing anything, so make an empty import object.
    let import_object = imports! {};
    let instance = Instance::new(module, &import_object)?;

    let add_one = instance.exports.get_function("add_one")?;
    let result = add_one.call([Value::I32(42)])?;
    assert_eq!(result[0], Value::I32(43));

    Ok(())
}
```

## Features

Wasmer is not only fast, but also designed to be highly customizable:
* **Pluggable Engines**: do you have a fancy `dlopen` implementation? This is for you!
* **Pluggable Compilers**: you want to emit code with DynASM or other compiler? We got you!
* **Headless mode**: that means that no compilers will be required
  to run a `serialized` Module (via `Module::deserialize()`).
* **Cross-compilation**: You can pre-compile a module and serialize it
  to then run it in other platform (via `Module::serialize()`).

## Config flags

Wasmer supports multiple features, from different engines to different compilers:
* `wat` (enabled by default): It allows to read WebAssembly files in their text format.
  *This feature is normally used only in development environments*
* Compilers (mutually exclusive):
  - `singlepass`: it will use `wasmer-compiler-singlepass` as the default
     compiler for compiling the WebAssembly module to machine code.
  - `cranelift`: it will use `wasmer-compiler-cranelift` as the default
     compiler for compiling the WebAssembly module to machine code.
  - `llvm`: it will use `wasmer-compiler-llvm` as the default
     compiler for compiling the WebAssembly module to machine code.

> Note: if you want to use multiple compilers at the same time, it's also possible!
> You will need to import them directly via each of the compiler crates.

---

Made with ❤️ by the Wasmer team, for the community
