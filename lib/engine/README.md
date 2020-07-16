# Wasmer Engine

The `wasmer-engine` crate is the general abstraction for creating Engines in Wasmer.

Wasmer Engines are mainly responsible for two things:
* Transform the compilation code (from any Wasmer Compiler) to **create** an `Artifact`
* **Load** an`Artifact` so it can be used by the user (normally, pushing the code into executable memory and so on)

It currently has two implementations:
* [JIT](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/engine-jit)
* [Native](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/engine-native)

## Example Implementation

Please check [`wasmer-engine-dummy`](../../tests/lib/engine-dummy/) for an example
implementation for an Engine.

### Acknowledgments

This project borrowed some of the code of the trap implementation from the [wasmtime-api](https://crates.io/crates/wasmtime), the code since then has evolved significantly.

Please check [Wasmer ATTRIBUTIONS](https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md) to further see licenses and other attributions of the project. 
