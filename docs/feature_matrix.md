# Feature Table

## Compiler Backend

| &nbsp; | Singlepass | Cranelift | LLVM |
| - | - | - | - |
| Caching | ❌ | ✅ | ✅ |
| SIMD | ❌ | ❌ | ✅ |
| Multi-value return | ❌ | ❌ | ❌ |
| Metering | ✅ | ✅ | ❓ | 


## Language integration

TODO: define a set of features that are relevant and mark them here

Current ideas:

- WASI FS API
- Callbacks
- Exiting early in hostcall
- Metering
- Caching
