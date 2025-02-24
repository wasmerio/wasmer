# `wasmer-wasi` [![Build Status](https://github.com/wasmerio/wasmer/actions/workflows/build.yml/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/main/LICENSE) [![crates.io](https://img.shields.io/crates/v/wasmer-wasi.svg)](https://crates.io/crates/wasmer-wasi)

This crate provides the necessary imports to use WASI easily from Wasmer.
[WebAssembly System Interface](https://github.com/WebAssembly/WASI)
(WASI for short) is a modular system interface for WebAssembly. WASI
is being standardized in the WebAssembly subgroup.

Very succinctly, from the user perspective, WASI is a set of
WebAssembly module _imports_ under a specific _namespace_ (which
varies based on the WASI version). A program compiled for the
`wasm32-wasip1` target will be able to support standard I/O, file I/O,
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
$ rustc --target wasm32-wasip1 hello.rs
```

Finally, let's execute it with the `wasmer` CLI:

```sh
$ wasmer run hello.wasm Gordon
Hello, Some("Gordon")
```

… and programatically with the `wasmer` and the `wasmer-wasi` libraries:

```rust
use std::io::Read;
use wasmer::{Module, Store};
use wasmer_wasix::{Pipe, WasiEnv};

let wasm_path = "hello.wasm";

// Let's declare the Wasm module with the text representation.
let wasm_bytes = std::fs::read(wasm_path)?;

// Create a Store.
let mut store = Store::default();

println!("Compiling module...");
// Let's compile the Wasm module.
let module = Module::new(&store, wasm_bytes)?;

let (stdout_tx, mut stdout_rx) = Pipe::channel();

// Run the module.
WasiEnv::builder("hello")
     .args(&["Gordon"])
    // .env("KEY", "Value")
    .stdout(Box::new(stdout_tx))
    .run_with_store(module, &mut store)?;

eprintln!("Run complete - reading output");

let mut buf = String::new();
stdout_rx.read_to_string(&mut buf).unwrap();

eprintln!("Output: {buf}");
```

Check the [fully working example using
WASI](https://github.com/wasmerio/wasmer/blob/main/examples/wasi.rs).

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
