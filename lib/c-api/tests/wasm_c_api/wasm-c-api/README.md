# WebAssembly C and C++ API 

Work in progress! No docs yet.


### Design Goals

* Provide a "black box" API for embedding a Wasm engine in other C/C++ applications.

  * Be completely agnostic to VM specifics.

  * Non-goal: "white box" interoperability with embedder (such as combined GC instead of mere finalisation) -- *much* more difficult to achieve.

* Allow creation of bindings for other languages through typical C foreign function interfaces.

  * Support a plain C API.

  * Stick to mostly manual memory management of interface objects.

* Avoid language features that raise barrier to use.

  * E.g., no exceptions or post-C++11 features in C++ API.

  * E.g., no passing of structs by-value or post-C99 features in C API.

* Achieve link-time compatibility between different implementations.

  * All implementation-dependent API classes are abstract and can be instantiated through factory methods only.


### Interfaces

* C++ API:

  * See `include/wasm.hh` for interface.

  * See `example/*.cc` for example usages.

* C API:

  * See `include/wasm.h` for interface.

  * See `example/*.c` for example usages.

Some random explanations:

* The VM must be initialised by creating an instance of an *engine* (`wasm::Engine`/`wasm_engine_t`) and is shut down by deleting it. Such an instance may only be created once per process.

* All runtime objects are tied to a specific *store* (`wasm::Store`/`wasm_store_t`). Multiple stores can be created, but their objects cannot interact. Every store and its objects must only be accessed in a single thread.

* To exchange module objects between threads, create a *shared* module (`wasm::Shared<Module>`/`wasm_shared_module_t`). Other objects cannot be shared in current Wasm.

* *Vector* structures (`wasm::vec<X>`/`wasm_x_vec_t`) are lightweight abstractions of a pair of a plain array and its length. The C++ API does not use `std::vector` because that does not support adopting pre-existing arrays.

* *References* point to runtime objects, but may involve internal indirections, which may or may not be cached. Thus, pointer equality on `Ref*` or subclasses cannot be used to compare identity of the underlying objects (`Ref::eq` may be added later). However, `nullptr`/`NULL` uniquely represents null references.

* The API already encompasses current proposals like [multiple return values](https://github.com/WebAssembly/multi-value/blob/master/proposals/multi-value/Overview.md) and [reference types](https://github.com/WebAssembly/reference-types/blob/master/proposals/reference-types/Overview.md), but not yet [threads](https://github.com/WebAssembly/threads/blob/master/proposals/threads/Overview.md).


### Prototype Implementation

* This repo contains a prototype implementation based on V8 is in `src`.

  * Note that this requires adding a module to V8, so it patches V8's build file.

* The C API is implemented on top of the C++ API.

* See `Makefile` for build recipe. Canonical steps to run examples:

  1. `make v8-checkout`
  2. `make v8`
  3. `make all`


#### Limitations

V8 implementation:

* Currently requires patching V8 by adding a module.

* Host functions (`Func::make`) create a JavaScript function internally, since V8 cannot handle raw C imports yet.

* As a consequence, does not support multiple results in external calls or host functions.

* Host functions and host globals are created through auxiliary modules constructed on the fly, to work around limitations in JS API.

* `Shared<Module>` is currently implemented via serialisation, since V8 does not currently have direct support for cross-isolate sharing.


### Other Implementations

Currently, known implementations of this API are included in

* V8 natively (both C and C++)
* Wabt (only C?)
* Wasmtime (only C?)
* [Wasmer](https://github.com/wasmerio/wasmer/tree/master/lib/c-api) (only C, C++ coming soon)


### TODO

Possible API tweaks:

  * Add `Ref::eq` (or better, a subclass `EqRef::eq`) for reference equality?

  * Add a way to return error messages from `Module::make` and `Module::validate`.

  * Use `restrict` in C API?

  * Find a way to perform C callbacks through C++ without extra wrapper?

  * Add iterators to `vec` class?
