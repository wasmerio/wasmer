# Feature Table

## Compiler Backend

| &nbsp; | Singlepass | Cranelift | LLVM |
| - | - | - | - |
| Caching | No | Yes | Yes |
| SIMD | No | No | Yes |
| Multi-value return | No | No | No |


## Language integration

TODO: define a set of features that are relevant and mark them here

Current ideas:

- WASI FS API
- Callbacks? (not even in Rust yet)
- Exiting early in hostcall
