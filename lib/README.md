# The Wasmer runtime crates

The crates can be grouped as follows.

* `api` — The public Rust API exposes everything a user needs to use Wasmer
  programatically through the `wasmer` crate,
* `c-api` — The public C API exposes everything a C user needs to use
  Wasmer programatically,
* `cache` — The traits and types to cache compiled WebAssembly
  modules,
* `cli` — The Wasmer CLI itself,
* `compiler` — The base for the compiler implementations, it defines
  the framework for the compilers and provides everything they need:
  * `compiler-cranelift` — A WebAssembly compiler based on the
    Cranelift compiler infrastructure,
  * `compiler-llvm` — A WebAssembly compiler based on the LLVM
    compiler infrastructure; recommended for runtime speed
    performance,
  * `compiler-singlepass` — A WebAssembly compiler based on our own
    compilation infrastructure; recommended for compilation-time speed
    performance.
* `deprecated` — The deprecated and old public Rust API, must not be
  used except if you don't want to migrate an old code using the 0.x
  version of Wasmer to 1.x,
* `derive` — A set of procedural macros used inside Wasmer,
* ABI:
  * `emscripten` — Emscripten ABI implementation inside Wasmer,
  * `wasi` — WASI ABI implementation inside Wasmer.
* `engine` — The general abstraction for creating an engine, which is
  responsible of leading the compiling and running flow:
  * `engine-universal` — 
  * `engine-dylib` — 
  * `engine-staticlib` — 
* `middlewares` —
* `types` —
* `vm` —

Extra crates:

* `object` —
* `wasi-experimental-io-devices` —
