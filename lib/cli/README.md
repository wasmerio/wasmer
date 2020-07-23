# Wasmer CLI

This crate is the Wasmer CLI.

The recommended way to install Wasmer is via your shell [following the instructions in our repo](https://github.com/wasmerio/wasmer-reborn#1-install-wasmer).

You can install it also via Cargo:

```bash
cargo install wasmer-cli
```

Or by running it inside the codebase:

```bash
cargo build --release
```

> Note: installing Wasmer via Cargo (or manual install) will not install
> the WAPM cli. If you want to use them together, please use the default shell installer.


## Features

The Wasmer supports the following features:
* `wat` (default): support for executing WebAssembly text files.
* `wast`(default): support for running wast test files.
* `jit` (default): support for the [JIT engine].
* `native` (default): support for the [Native engine].
* `cache` (default): support or automatically caching compiled artifacts.
* `wasi` (default): support for [WASI].
* `emscripten` (default): support for [Emscripten].
* `singlepass`: support for the [Singlepass compiler].
* `cranelift`: support for the [Cranelift compiler].
* `llvm`: support for the [LLVM compiler].

[JIT Engine]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/engine-jit/
[Native Engine]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/engine-native/
[WASI]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/wasi/
[Emscripten]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/emscripten/
[Singlepass compiler]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-singlepass/
[Cranelift compiler]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-cranelift/
[LLVM compiler]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-llvm/

## CLI commands

Once you have Wasmer installed, you can start executing WebAssembly files easily:

Get the current Wasmer version:

```bash
wasmer -V
```

Execute a WebAssembly file:

```bash
wasmer run myfile.wasm
```

Compile a WebAssembly file:

```bash
wasmer compile myfile.wasm -o myfile.so --native
```

Run a compiled WebAssembly file (fastest):

```bash
wasmer run myfile.so
```
