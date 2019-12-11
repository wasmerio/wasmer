# Feature Table

## Compiler Backend

| &nbsp; | Singlepass | Cranelift | LLVM |
| - | :-: | :-: | :-: |
| Caching | ✅ | ✅ | ✅ |
| Emscripten | ✅ | ✅ | ✅ |
| Metering | ✅ | ⬜ | ✅ |
| Multi-value return | ⬜ | ⬜ | ⬜ |
| OSR | 🔄 | ⬜ | 🔄 |
| SIMD | ⬜ | ⬜ | ✅ |
| WASI | ✅ | ✅ | ✅ |
| WASMER_BACKTRACE | ✅ | ⬜ | ⬜ |

## Operating System
| &nbsp; | GNU Linux | Mac OSX | Windows NT |
| - | :-: | :-: | :-: |
| Cranelift Backend | ✅ | ✅ | ✅ |
| LLVM Backend | ✅ | ✅ | ✅ |
| Singlepass Backend | ✅ | ✅ | [#347](https://github.com/wasmerio/wasmer/issues/347) | 
| WASI | ✅ | ✅ | ✅* |

* `poll_fd` is not fully implemented for Windows yet

## Language integration

TODO: define a set of features that are relevant and mark them here

Current ideas:

- Callbacks
- Metering
- Caching

> TODO: expand this table, it's focused on new features that we haven't implemented yet and doesn't list all language integrations

| &nbsp; | Rust | C / C++ | Go | Python | Ruby |
| - | :-: | :-: | :-: | :-: | :-: |
| Terminate in host call | ✅ | ⬜ | ⬜ | ⬜ | ⬜ |
| WASI | ✅ | ✅ | 🔄 | ⬜ | ⬜ | 
| WASI FS API | ✅ | ⬜ | ⬜ | ⬜ | ⬜ |
