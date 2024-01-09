# `wasmer-wasi` [![Build Status](https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE) [![crates.io](https://img.shields.io/crates/v/wasmer-wasi.svg)](https://crates.io/crates/wasmer-wasi)

This crate provides the necessary imports to use WASI easily from Wasmer.
[WebAssembly System Interface](https://github.com/WebAssembly/WASI)
(WASI for short) is a modular system interface for WebAssembly. WASI
is being standardized in the WebAssembly subgroup.

Very succinctly, from the user perspective, WASI is a set of
WebAssembly module _imports_ under a specific _namespace_ (which
varies based on the WASI version). A program compiled for the
`wasm32-wasi` target will be able to support standard I/O, file I/O,
filesystem manipulation, memory management, time, string, environment
variables, program startup etc.

Wasmer WASI is created with the aim to be fully sandboxed.
We are able to achieve that thanks to our Virtual Filesystem implementation (`wasmer-vfs`)
and by only allowing secure systemcalls back to the host.

> Note: If you encounter any sandboxing issue please open an issue in the wasmer repo https://github.com/wasmerio/wasmer.

This crate provides the necessary API to create the imports to use
WASI easily from the Wasmer runtime, through our `ImportObject` API.

## Supported WASI versions

| WASI version             | Support |
| ------------------------ | ------- |
| `wasi_unstable`          | ✅       |
| `wasi_snapshot_preview1` | ✅       |

The special `Latest` version points to `wasi_snapshot_preview1`.

Learn more about [the WASI version
process](https://github.com/WebAssembly/WASI/tree/main/phases)
(ephemeral, snapshot, old).

## Usage

Let's consider the following `hello.rs` Rust program:

```rust
fn main() {
    println!("Hello, {:?}", std::env::args().nth(1));
}
```

Then, let's compile it to a WebAssembly module with WASI support:

```sh
$ rustc --target wasm32-wasi hello.rs
```

Finally, let's execute it with the `wasmer` CLI:

```sh
$ wasmer run hello.wasm Gordon
Hello, Some("Gordon")
```

… and programatically with the `wasmer` and the `wasmer-wasi` libraries:

```rust
use wasmer::{Store, Module, Instance};
use wasmer_wasix::WasiState;

let mut store = Store::default();
let module = Module::from_file(&store, "hello.wasm")?;

// Create the `WasiEnv`.
let wasi_env = WasiState::builder("command-name")
    .args(&["Gordon"])
    .finalize()?;

// Generate an `ImportObject`.
let mut wasi_thread = wasi_env.new_thread();
let import_object = wasi_thread.import_object(&module)?;

// Let's instantiate the module with the imports.
let instance = Instance::new(&module, &import_object)?;

// Let's call the `_start` function, which is our `main` function in Rust.
let start = instance.exports.get_function("_start")?;
start.call(&[])?;
```

Check the [fully working example using
WASI](https://github.com/wasmerio/wasmer/blob/master/examples/wasi.rs).

## More resources

This library is about the WASI implementation inside the Wasmer
runtime. It contains documentation about the implementation
itself. However, if you wish to learn more about WASI, here is a list
of links that may help you:

* [`wasi-libc`](https://github.com/WebAssembly/wasi-libc/), WASI libc
  implementation for WebAssembly programs built on top of WASI system
  calls. It provides a wide array of POSIX-compatible C APIs,
  including support for standard I/O, file I/O, filesystem
  manipulation, memory management, time, string, environment
  variables, program startup, and many other APIs,
* [`wasi-sdk`](https://github.com/WebAssembly/wasi-sdk/), WASI-enabled
  WebAssembly C/C++ toolchain, based on `wasi-libc`,
* [WASI API
  documentation](https://github.com/WebAssembly/WASI/blob/main/phases/snapshot/docs.md),
* [WASI C API header
  file](https://github.com/WebAssembly/wasi-libc/blob/main/libc-bottom-half/headers/public/wasi/api.h),
* [WASI Application Binary Interface
  (ABI)](https://github.com/WebAssembly/WASI/blob/main/legacy/application-abi.md),
  where we learn about `_start` and `_initialize` (for _reactors_) for example.
