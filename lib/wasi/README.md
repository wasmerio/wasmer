# `wasmer-wasi` [![Build Status](https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

This crate provides the necessary imports to use WASI easily from Wasmer.

## Usage

First, add this crate into your `Cargo.toml` dependencies:

```toml
wasmer-wasi = "1.0.0-alpha"
```

And then:

```rust
use wasmer::{Store, Module, Instance};
use wasmer_wasi::WasiState;

let store = Store::default();
let module = Module::from_file(&store, "my_wasi_module.wasm")?;

// Create the WasiEnv
let wasi_env = WasiState::new("command name")
    .args(&["world"])
    .env("KEY", "VALUE")
    .finalize()?;

let import_object = wasi_env.import_object(&module)?;
let instance = Instance::new(&module, &import_object)?;

wasi_env.set_memory(instance.exports.get_memory("memory")?.clone());

let start = instance.exports.get_function("_start")?;
start.call(&[])?;
```

*Note: you can find a [full working example using WASI here](https://github.com/wasmerio/wasmer/blob/master/examples/wasi.rs).*
