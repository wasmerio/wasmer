# Wasmer Engine - JIT

The Wasmer JIT engine is usable with any compiler implementation based
on [`wasmer-compiler`]. After the compiler process the result, the JIT
pushes it into memory and links its contents so it can be usable by
the [`wasmer`] API.

*Note: you can find a [full working example using the JIT engine
here][example].*

### Acknowledgments

This project borrowed some of the code of the code memory and unwind
tables from the [`wasmtime-jit`], the code since then has evolved
significantly.

Please check [Wasmer `ATTRIBUTIONS`] to further see licenses and other
attributions of the project.


[`wasmer-compiler`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler
[`wasmer`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/api
[example]: https://github.com/wasmerio/wasmer-reborn/blob/master/examples/engine_jit.rs
[`wasmtime-jit`]: https://crates.io/crates/wasmtime-jit
[Wasmer `ATTRIBUTIONS`]: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md
