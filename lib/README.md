# The Wasmer runtime crates

The philosophy of Wasmer is to be very modular by design. It's
composed of a set of crates. We can group them as follows:

* `api` — The public Rust or JS API exposes everything a user needs to use Wasmer
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
* `derive` — A set of procedural macros used inside Wasmer,
* ABI:
  * `emscripten` — Emscripten ABI implementation inside Wasmer,
  * `wasi` — WASI ABI implementation inside Wasmer:
    * `wasi-types` — All the WASI types,
* `engine` — The general abstraction for creating an engine, which is
  responsible of leading the compiling and running flow. Using the
  same compiler, the runtime performance will be approximately the
  same, however the way it stores and loads the executable code will
  differ:
  * `engine-universal` — stores the code in a custom file format, and
    loads it in memory,
  * `object` — A library to cross-generate native objects for various
    platforms.
* `middlewares` — A collection of middlewares, like `metering` that
  tracks how many operators are executed in total and putting a limit
  on the total number of operators executed,
* `types` — The basic structures to use WebAssembly,
* `vm` — The Wasmer VM runtime library, the low-level base of
  everything.
