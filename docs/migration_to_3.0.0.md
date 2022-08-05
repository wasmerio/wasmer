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

### Creating a function environment (function state)

You need a `Store` to create an environment. It is created like this:

```rust
let env = FunctionEnv::new(&mut store, ()); // Empty environment.
```

, or

```rust
let my_counter = 0_i32;
let env = FunctionEnv::new(&mut store, my_counter);
```

Any type can be passed as the environment: (*Nota bene* the passed type `T` must implement the `Any` trait, that is, any type which contains a non-`'static` reference.)

```rust
struct Env {
    counter: i32,
}
let env = FunctionEnv::new(&mut store, Env {counter: 0});
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

For WASI, don't forget to import memory to `WasiEnv`

```rust
let mut wasi_env = WasiState::new("hello").finalize()?;
let import_object = wasi_env.import_object(&mut store, &module)?;
let instance = Instance::new(&mut store, &module, &import_object).expect("Could not instantiate module.");
let memory = instance.exports.get_memory("memory")?;
wasi_env.data_mut(&mut store).set_memory(memory.clone());
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
