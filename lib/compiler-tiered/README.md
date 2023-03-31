# `wasmer-compiler-tiered`

This crate contains a compiler implementation of a tiered compilation workflow
where first `singlepass` is used and then once `cranelift` is compiled in the
background the module will automatically switch over

## Usage

```rust
use wasmer::{Store, EngineBuilder};
use wasmer_compiler_tiered::Tiered;

let compiler = Tiered::new();
let mut store = Store::new(compiler);
```
