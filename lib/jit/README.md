# Wasmer JIT

The Wasmer JIT is usable with any compiler implementation
based on `wasmer-compiler`.
After the compiler process the result, the JIT pushes it into
memory and links it's contents so it can be usable by the
`wasmer` api.

#### Aknowledgments

This project borrowed some of the code of the code memory and unwind tables from the [wasmtime-jit](https://crates.io/crates/wasmtime-jit), the code since then has evolved significantly.

Please check [Wasmer ATTRIBUTIONS](https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md) to further see licenses and other attributions of the project. 
