# Wasmer Compiler - Cranelift

This is the `wasmer-compiler-cranelift` crate, which contains a
compiler implementation based on Cranelift.

## Usage

First, add this crate into your `Cargo.toml` dependencies:

```toml
wasmer-compiler-cranelift = "1.0.0-alpha.1"
```

And then:

```rust
use wasmer::{Store, JIT};
use wasmer_compiler_cranelift::Cranelift;

let compiler = Cranelift::new();
// Put it into an engine and add it to the store
let store = Store::new(&JIT::new(&compiler).engine());
```

*Note: you can find a [full working example using Cranelift compiler here](https://github.com/wasmerio/wasmer-reborn/blob/test-examples/examples/compiler-cranelift.rs).*

## When to use Cranelift

We recommend using this compiler crate **only for development proposes**.
For production we recommend using `wasmer-compiler-llvm` as it offers
a much better runtime speed (50% faster on average).

### Acknowledgments

This project borrowed some of the function lowering from [cranelift-wasm](https://crates.io/crates/cranelift-wasm).

Please check [Wasmer ATTRIBUTIONS](https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md) to further see licenses and other attributions of the project. 
