# `wasmer-engine-jit` [![Build Status](https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square)](https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

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
