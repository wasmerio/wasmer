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
2. There is a new `Context` that goes along the `Store`. The `Context` will keep track of all memory and functions used, removing old tracking and Weak/Strong pointer difference. Every function and memory that is retreive is linked to a specific `Context`, and cannot be mixed with another `Context`
3. `WasmerEnv` and associated traits and macro have been removed. To use Environenment, a simple structure now need to be attached to the `Context`, and can be retreive from current `Context`. All functions now takes a mendatory first argument that is of type `ContextMut<_>`, with `_` being either nothing `()` or the Environement type needed. the function creation `XXX_with_env(...)` don't exist anymore, simply use `Function::new(...)` or `Function::native_new(...)` with the correct `ContextMut<_>` type. Because the `WasmerEnv` and all helpers don't exists anymore, you have to import memory yourself, there isn't any per instance initialisation automatically done anymore. It's especialy important for WasiEnv context. `Env` can be accessed from a `Context` using `Context::data()` or `Context::data_mut()`.
4. The `Engine`s API has been simplified, Instead of the `wasmer` user choosing and setting up an engine explicitly, everything now uses the universal engine. All functionalites of the `staticlib`,`dylib` Engines should be available unless explicitly stated as unsupported.

## How to use Wasmer 3.0.0

### Installing Wasmer CLI

See [wasmer.io] for installation instructions.

If you already have wasmer installed, run `wasmer self-update`.

Install the latest versions of Wasmer with [wasmer-nightly] or by following the
steps described in the documentation: [Getting Started][getting-started].

### Using Wasmer 3.0.0

See the [examples] to find out how to do specific things in Wasmer 3.0.0.

## Project Structure

TODO

## Differences

### Creating a Context

You need a Store to create a context. Simple context is created using:

```rust
let ctx = FunctionEnv::new(&mut store, ());
```

For a Context with a custom Env, it will be similar:

```rust
#[derive(Clone)]
struct Env {
    counter: i32,
}
let ctx = FunctionEnv::new(&mut store, Env{counter: 0});
```

### Managing imports

Instantiating a Wasm module is similar to 2.x with just the need of a context as difference:

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

For WASI, don't forget to initialize it

```rust
let mut wasi_env = WasiState::new("hello").finalize()?;
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

In Wasmer 3.0, there's only the universal engine. The user can ignore the engine details when using the API:

```rust
let wasm_bytes = wat2wasm(
    "..".as_bytes(),
)?;

let compiler_config = Cranelift::default();
let mut store = Store::new(&compiler_config);
let module = Module::new(&store, wasm_bytes)?;
let instance = Instance::new(&module, &imports! {})?;
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
