# Wasmer Engine

The `wasmer-engine` crate is the general abstraction for creating Engines in Wasmer.

Wasmer Engines are mainly responsible for two things:
* Transform the compilation code (from any Wasmer Compiler) to
  **create** an `Artifact`,
* **Load** an`Artifact` so it can be used by the user (normally,
  pushing the code into executable memory and so on).

It currently has two implementations:
1. JIT with [`wasmer-engine-jit`],
2. Native with [`wasmer-engine-native`].

## Example Implementation

Please check [`wasmer-engine-dummy`] for an example implementation for
an `Engine`.

### Acknowledgments

This project borrowed some of the code of the trap implementation from
the [`wasmtime-api`], the code since then has evolved significantly.

Please check [Wasmer `ATTRIBUTIONS`] to further see licenses and other
attributions of the project.


[`wasmer-engine-jit`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/engine-jit
[`wasmer-engine-native`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/engine-native
[`wasmer-engine-dummy`]: https://github.com/wasmerio/wasmer-reborn/tree/master/tests/lib/engine-dummy
[`wasmtime-api`]: https://crates.io/crates/wasmtime
[Wasmer `ATTRIBUTIONS`]: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md
