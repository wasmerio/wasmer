<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master">
    <img src="https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square" alt="Build Status">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
  <a href="https://crates.io/crates/wasmer-spectests">
    <img src="https://img.shields.io/crates/d/wasmer-spectests.svg?style=flat-square" alt="Number of downloads from crates.io">
  </a>
  <a href="https://docs.rs/wasmer-spectests">
    <img src="https://docs.rs/wasmer-spectests/badge.svg" alt="Read our API documentation">
  </a>
</p>

# Wasmer Spectests

Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
compatible with Emscripten, Rust and Go. [Learn
more](https://github.com/wasmerio/wasmer).

This crate allows to test the Wasmer runtime against the official
specification test suite.

This lib contains tests for the core WebAssembly semantics, as described in [Semantics.md](https://github.com/WebAssembly/design/blob/master/Semantics.md) and specified by the [spec interpreter](https://github.com/WebAssembly/spec/blob/master/interpreter/spec).

SIMD wast specs are also added here.

These files should be a direct copy of the original [WebAssembly spec tests](/test/core).

Tests are written in the [S-Expression script format](https://github.com/WebAssembly/spec/blob/master/interpreter/README.md#s-expression-syntax) defined by the interpreter.

## Version
The spectests were last updated at `WebAssembly/spec` commit `a221f2574d7106e92cf8abaf05d5bb1131b19d76`.

## Testcases

Currently supported command assertions:

- [x] `module` _fully implemented_
- [x] `assert_return` _fully implemented_
- [x] `assert_return_canonical_nan` _fully implemented_
- [x] `assert_return_arithmetic_nan` _fully implemented_
- [x] `assert_trap` _fully implemented_
- [x] `assert_invalid` _fully implemented_ (it should not require validation to be performed separate from compilation)
- [x] `assert_malformed` _fully implemented_
- [ ] `assert_uninstantiable` _not implemented, no usages found_
- [x] `assert_exhaustion` _fully implemented_
- [x] `register` _fully implemented_
- [x] `perform_action` _fully implemented_

### Covered spec tests
See `tests/excludes.txt` for current coverage.

### Specific non-supported cases

There are some cases that we decided to skip for now to accelerate the release schedule:

- `SKIP_CALL_INDIRECT_TYPE_MISMATCH`: we implemented traps in a fast way. We haven't yet covered the type mismatch on `call_indirect`. Specs affected:

  - `call_indirect.wast`

- `SKIP_CALL_UNDEFINED_ELEMENT`
  Tables are imported into every spec module, even for modules that don't expect it. We need to figure out a way to prevent importing of objects that are not explicitly imported into the module.

Currently `cranelift_wasm::ModuleEnvironment` does not provide `declare_table_import`, etc. so there is no meaningful way of fixing this yet.

- `call_indirect.wast`

- `SKIP_SHARED_TABLE` [elem.wast]
  Currently sharing tables between instances/modules does not work. Below are some of the reasons it is hard to achieve:

  - Rust naturally prevents such because of the possibility of race conditions
  - `ImportObject` is just a wrapper, what we really care about is references to its content.
  - `Instance::new` contains a mutation points, the part where after getting the object (memory or table) we push values to it
    `table[table_element_index] = func_addr`
  - Instance has its own created memories and tables and references to them must outlive `Instance::new()`
  - Possible strategy:

    ```rust
    // ImportObject should be passed by ref
    Instance::new<'a>(..., &ImportObject);

    // Add OwnedData to Instance struct
    struct OwnedData;

    // For parts where mutatation is really needed
    fn get_mut(&import) -> &mut ImportObject {
        unsafe { transmute::<&ImportObject, &mut ImportObject>(import) }
    }
    ```

  - `elem.wast`

- `SKIP_UNARY_OPERATION` [memory_grow.wast]
  In some versions of MacOS this is failing (perhaps because of the chip).
  More info here: 
 ```
Executing function c82_l299_action_invoke
thread 'test_memory_grow::test_module_5' panicked at 'assertion failed: `(left == right)`
  left: `Ok([I32(0)])`,
 right: `Ok([I32(31)])`', /Users/distiller/project/target/release/build/wasmer-spectests-98805f54de053dd1/out/spectests.rs:32304:5
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace.


failures:
    test_memory_grow::test_module_5
```
  https://circleci.com/gh/wasmerio/wasmer/9556
  
### Development
To test locally, try the following commands:

```
RUST_BACKTRACE=1 cargo test --features clif -- --nocapture
RUST_BACKTRACE=1 cargo test --features llvm -- --nocapture
RUST_BACKTRACE=1 cargo +nightly test --features singlepass -- --nocapture
```