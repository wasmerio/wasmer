# Feature Table

## Compiler Backend

| &nbsp; | Singlepass | Cranelift | LLVM |
| - | :-: | :-: | :-: |
| Caching | 🥚 | 🐔 | 🐔 |
| Emscripten | 🐔 | 🐔 | 🐔 |
| Metering | 🐔 | 🥚 | 🐔 |
| Multi-value return | 🥚 | 🥚 | 🥚 |
| OSR | 🐤 | ❓ | ❓ |
| SIMD | 🥚 | 🥚  | 🐔 |
| WASI | 🐔 | 🐔 | 🐔 |

Key:

🥚 - Not yet started

🐣 - Implementation or design started

🐤 - In progress or partial support

🐔 - Complete



## Language integration

TODO: define a set of features that are relevant and mark them here

Current ideas:

- WASI FS API
- Callbacks
- Exiting early in hostcall
- Metering
- Caching
