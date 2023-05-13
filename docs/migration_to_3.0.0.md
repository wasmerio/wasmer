# Migrating from Wasmer 2.x to Wasmer 3.0.0

This document will describe the differences between Wasmer 2.x and Wasmer 3.0.0
and provide examples to make migrating to the new API as simple as possible.

## Table of Contents

- [Rationale for changes in 3.0.0](#rationale-for-changes-in-300)
- [How to use Wasmer 3.0.0](#how-to-use-wasmer-300)
  - [Installing Wasmer CLI](#installing-wamser-cli)
  - [Using Wasmer 3.0.0](#using-wamser-300)
- [Project structure](#project-structure)
- [Differences](#differences)
  - [Managing imports](#managing-imports)
  - [Engines](#engines)
  - [C-API changes](#c-api)

## Rationale for changes in 3.0.0

This version introduces the following changes to make the Wasmer API more ergonomic and safe:

1. `ImportsObject` and the traits `Resolver`, `NamedResolver`, etc have been removed and replaced with a single simple type `Imports`. This reduces the complexity of setting up an `Instance`. The helper macro `imports!` can still be used.
2. The `Store` will keep track of all memory and functions used, removing old tracking and Weak/Strong pointer usage. Every function and memory that can be defined is associated to a specific `Store`, and cannot be mixed with another `Store`
3. `NativeFunc` has been renamed to `TypedFunction`, accordingly the following functions have been renamed:
   * `Function::native(…)` → `Function::typed(…)`
   * `Function::new_native(…)` → `Function::new_typed(…)`
   * `Function::new_native_with_env(…)` → `Function::new_typed_with_env(…)`
   The previous variants still exist in order to support the migration, but they have been deprecated.
4. `WasmerEnv` and associated traits and macro have been removed. To use a function environment, you will need to create a `FunctionEnv` object and pass it along when you construct the function. For convenience, these functions also exist in a variant without the environment for simpler use cases that don't need it. For the variants with the environment, it can be retrieved from the first argument of the function. Because the `WasmerEnv` and all helpers don't exists anymore, you have to import memory yourself, there isn't any per instance initialisation automatically done anymore. It's especially important in wasi use with `WasiEnv`. `Env` can be accessed from a `FunctionEnvMut<'_, WasiEnv>` using `FunctionEnvMut::data()` or `FunctionEnvMut::data_mut()`.
5. The `Engine`s API has been simplified, Instead of the user choosing and setting up an engine explicitly, everything now uses a single engine. All functionalities of the `universal`, `staticlib` and `dylib` engines should be available in this new engine unless explicitly stated as unsupported.
6. Loupe has been removed. Memory inspection should be done manually

## How to use Wasmer 3.0.0

### Installing Wasmer CLI

See [wasmer.io] for installation instructions.

If you already have wasmer installed, run `wasmer self-update`.

Install the latest versions of Wasmer with [wasmer-nightly] or by following the
steps described in the documentation: [Getting Started][getting-started].

### Using Wasmer 3.0.0

One of the main changes in 3.0.0 is that `Store` now owns all WebAssembly objects; thus exports like a `Memory` are merely handles to the actual memory object inside the store. To read/write any such value you will always need a `Store` reference.

If you define your own function, when the function is called it will hence need a reference to the store in order to access WebAssembly objects. This is achieved by the `StoreRef<'_>` and `StoreMut<'_>` types, which borrow from the store and provide access to its data. Furthermore, to prevent borrowing issues you can create new `StoreRef` and `StoreMut`s whenever you need to pass one at another function. This is done with the `AsStoreRef`, `AsStoreMut` traits.

See the [examples] to find out how to do specific things in Wasmer 3.0.0.

## Project Structure

A lot of types were moved to `wasmer-types` crate. There are no `engine` crates anymore; all the logic is included in `wasmer-compiler`.

## Differences

### `WasmerEnv` is removed in favor of `FunctionEnv`

`WasmerEnv` has been removed in Wasmer 3.0 in favor of `FunctionEnv`, which is now shareable automatically between functions without requiring the environment to be clonable.

```rust
let my_counter = 0_i32;
let env = FunctionEnv::new(&mut store, my_counter);
```

Note: Any type can be passed as the environment: (*Nota bene* the passed type `T` must implement the `Any` trait, that is, any type which contains a non-`'static` reference.)

```rust
struct Env {
    counter: i32,
}
let env = FunctionEnv::new(&mut store, Env {counter: 0});
```

Here's how the code depending on `WasmerEnv` should evolve:

#### Before

```rust
#[derive(wasmer::WasmerEnv, Clone)]
pub struct MyEnv {
    #[wasmer(export)]
    pub memory: wasmer::LazyInit<Memory>,
    #[wasmer(export(name = "__alloc"))]
    pub alloc_guest_memory: LazyInit<NativeFunc<u32, i32>>,

    pub multiply_by: u32,
}

let my_env = MyEnv {
  memory: Default::default(),
  alloc_guest_memory: Default::default(),
  multiply_by: 10,
};

let instance = Instance::new(&module, &imports);
```

#### After

```rust
pub struct MyEnv {
    pub memory: Option<Memory>,
    pub alloc_guest_memory: Option<TypedFunction<i32, i32>>,
    pub multiply_by: u32,
}

let env = FunctionEnv::new(&mut store, MyEnv {
  memory: None,
  alloc_guest_memory: None,
  multiply_by: 10,
});

let instance = Instance::new(&mut store, &module, &imports)?;
let mut env_mut = env.into_mut(&mut store); // change to a FunctionEnvMut
let (mut data_mut, mut store_mut) = env_mut.data_and_store_mut(); // grab data and a new store_mut
data_mut.memory = Some(instance.exports.get_memory("memory")?.clone());
data_mut.alloc_guest_memory = Some(instance.exports.get_typed_function(&mut store_mut, "__alloc")?);
```

### New `MemoryView` API (preparation for shared memory)

Reading from memory has slightly changed compared to 2.x:

```rust
// 2.x
let memory = instance.exports.get_memory("mem")?;
println!("Memory size (pages) {:?}", memory.size());
println!("Memory size (bytes) {:?}", memory.data_size());

let load = instance
    .exports
    .get_native_function::<(), (WasmPtr<u8, Array>, i32)>("load")?;

let (ptr, length) = load.call(&mut store)?;
let str = ptr.get_utf8_string(memory, length as u32).unwrap();
println!("Memory contents: {:?}", str);
```

```rust
// 3.x
let memory = instance.exports.get_memory("mem")?;
let memory_view = memory.view(&store);
println!("Memory size (pages) {:?}", memory_view.size());
println!("Memory size (bytes) {:?}", memory_view.data_size());

let load: TypedFunction<(), (WasmPtr<u8>, i32)> =
    instance.exports.get_typed_function(&mut store, "load")?;

let (ptr, length) = load.call(&mut store)?;
let memory_view = memory.view(&store);
let str = ptr.read_utf8_string(&memory_view, length as u32).unwrap();
println!("Memory contents: {:?}", str);
```

The reason for this change is that in the future this will enable
safely sharing memory across threads. The same thing goes for reading slices:

```rust
// 2.x
let new_str = b"Hello, Wasmer!";
let values = ptr.deref(memory, 0, new_str.len() as u32).unwrap();
for i in 0..new_str.len() {
    values[i].set(new_str[i]);
}
```

```rust
// 3.x
let memory_view = memory.view(&store); // (can be reused)
let new_str = b"Hello, Wasmer!";
let values = ptr.slice(&memory_view, new_str.len() as u32).unwrap();
for i in 0..new_str.len() {
    values.index(i as u64).write(new_str[i]).unwrap();
}
```

### Managing imports

Instantiating a Wasm module is similar to 2.x;

```rust
let import_object: Imports = imports! {
    "env" => {
        "host_function" => host_function,
    },
};
let instance = Instance::new(&mut store, &module, &import_object).expect("Could not instantiate module.");
```

You can also build the `Imports` object manually:

```rust
let mut import_object: Imports = Imports::new();
import_object.define("env", "host_function", host_function);
let instance = Instance::new(&mut store, &module, &import_object).expect("Could not instantiate module.");
```

For WASI, don't forget to initialize the `WasiEnv` (it will import the memory)

```rust
let mut wasi_env = WasiState::builder("hello").finalize()?;
let import_object = wasi_env.import_object(&mut store, &module)?;
let instance = Instance::new(&mut store, &module, &import_object).expect("Could not instantiate module.");
wasi_env.initialize(&mut store, &instance).unwrap();
```

#### `ChainableNamedResolver` is removed

Chaining imports with a trait has been deemed too complex for what it does; it's possible to chain (i.e. override) an `Imports`' contents by using its implementation of `std::iter::Extend`  from the Rust standard library:

```rust
let imports1: Imports = todo!();
let mut imports2: Imports = todo!();

imports2.extend(&imports);
// This is equivalent to the following:
// for ((ns, name), ext) in imports1.into_iter() {
//     imports2.define(&ns &name, ext);
// }
```

### Engines

#### Before

In Wasmer 2.0, you had to explicitly define the Engine you want to use:

```rust
let wasm_bytes = wat2wasm(
    "..".as_bytes(),
)?;

let compiler_config = Cranelift::default();
let engine = Universal::new(compiler_config).engine();
let mut store = Store::new(&engine);
let module = Module::new(&store, wasm_bytes)?;
let instance = Instance::new(&module, &imports! {})?;
```

#### After

In Wasmer 3.0, there's only one engine. The user can ignore the engine details when using the API:

```rust
let wasm_bytes = wat2wasm(
    "..".as_bytes(),
)?;

let compiler = Cranelift::default();
let mut store = Store::new(compiler);
let module = Module::new(&store, wasm_bytes)?;
let instance = Instance::new(&mut store, &module, &imports! {})?;
```

#### Advanced configuration

The previous ability to define target and features remains in a new `EngineBuilder` interface:

```rust
let compiler = Cranelift::default();

let mut features = Features::new();
// Enable the multi-value feature.
features.multi_value(true);

let engine = EngineBuilder::new(compiler).set_features(Some(features));
let store = Store::new(engine);
```

### C-API

The WASM C-API hasn't changed. Some wasmer-specific functions have changed, that relate to setting up WASI environments.

- `wasi_env_new` function changed input parameters to accommodate the new Store API, it now is:
  ```C
  struct wasi_env_t *wasi_env_new(wasm_store_t *store, struct wasi_config_t *config);
  ```
- `wasi_get_imports` function changed input parameters to accommodate the new Store API, it now is:
  ```c
  bool wasi_get_imports(const wasm_store_t *_store,
                        struct wasi_env_t *wasi_env,
                        const wasm_module_t *module,
                        wasm_extern_vec_t *imports);
  ```
- `wasi_env_set_memory` was added. It's necessary to set the `WasiEnv` memory by getting it from `Instance`s memory exports after its initialization. This must be performed in a specific order:
  1. Create WasiEnv
  2. Create Instance
  3. Get Instance Exports
  4. Find Memory from Instance Exports and store it to WasiEnv
  The function's signature is:
  ```c
  void wasi_env_set_memory(struct wasi_env_t *env, const wasm_memory_t *memory);
  ```

[examples]: https://docs.wasmer.io/integrations/examples
[wasmer]: https://crates.io/crates/wasmer
[wasmer-wasi]: https://crates.io/crates/wasmer-wasi
[wasmer-emscripten]: https://crates.io/crates/wasmer-emscripten
[wasmer-compiler]: https://crates.io/crates/wasmer-compiler
[wasmer.io]: https://wasmer.io
[wasmer-nightly]: https://github.com/wasmerio/wasmer-nightly/
[getting-started]: https://docs.wasmer.io/ecosystem/wasmer/getting-started
[instance-example]: https://docs.wasmer.io/integrations/examples/instance
[imports-exports-example]: https://docs.wasmer.io/integrations/examples/imports-and-exports
[host-functions-example]: https://docs.wasmer.io/integrations/examples/host-functions
[memory]: https://docs.wasmer.io/integrations/examples/memory
[memory-pointers]: https://docs.wasmer.io/integrations/examples/memory-pointers
[host-functions]: https://docs.wasmer.io/integrations/examples/host-functions
[errors]: https://docs.wasmer.io/integrations/examples/errors
[exit-early]: https://docs.wasmer.io/integrations/examples/exit-early
