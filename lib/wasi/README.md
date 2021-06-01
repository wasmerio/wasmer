<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-wasi</code> library</h1>

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
    <a href="https://crates.io/crates/wasmer-wasi">
      <img src="https://img.shields.io/crates/v/wasmer-wasi.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_wasi/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

This crate provides the necessary imports to use WASI easily from Wasmer.

## Usage

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

let start = instance.exports.get_function("_start")?;
start.call(&[])?;
```

*Note: you can find a [full working example using WASI here](https://github.com/wasmerio/wasmer/blob/master/examples/wasi.rs).*
