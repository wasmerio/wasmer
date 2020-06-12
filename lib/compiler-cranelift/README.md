# Wasmer Compiler - Cranelift

This is the `wasmer-compiler-cranelift` crate, which contains a
compiler implementation based on Cranelift.

We recommend using this compiler only for development proposes.
For production we recommend using `wasmer-compiler-llvm` as it offers
a much better runtime speed (50% faster on average).

### Acknowledgments

This project borrowed some of the function lowering from [cranelift-wasm](https://crates.io/crates/cranelift-wasm).

Please check [Wasmer ATTRIBUTIONS](https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md) to further see licenses and other attributions of the project. 
