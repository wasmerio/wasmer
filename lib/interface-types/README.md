<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master">
    <img src="https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square" alt="Build Status">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
  <a href="https://crates.io/crates/wasmer-interface-types">
    <img src="https://img.shields.io/crates/d/wasmer-interface-types.svg?style=flat-square" alt="Number of downloads from crates.io">
  </a>
  <a href="https://docs.rs/wasmer-interface-types">
    <img src="https://docs.rs/wasmer-interface-types/badge.svg" alt="Read our API documentation">
  </a>
</p>

# Wasmer Interface Types

Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
compatible with WASI, Emscripten, Rust and Go. [Learn
more](https://github.com/wasmerio/wasmer).

This crate is an implementation of [the living WebAssembly Interface
Types standard](https://github.com/WebAssembly/interface-types).

## Encoders and decoders

The `wasmer-interface-types` crate comes with an encoder and a decoder
for the WAT format, and the binary format, for the WebAssembly
Interface Types. An encoder writes an AST into another format, like
WAT or binary. A decoder reads an AST from another format, like WAT or
binary.

## Instructions

Very basically, WebAssembly Interface Types defines a [set of
instructions](https://github.com/WebAssembly/interface-types/blob/master/proposals/interface-types/working-notes/Instructions.md),
used by adapters to transform the data between WebAssembly core and
the outside world ([learn
mode](https://github.com/WebAssembly/interface-types/blob/master/proposals/interface-types/Explainer.md)).

Here is the instructions that are implemented by this crate:

| Instruction | WAT encoder | Binary encoder | WAT decoder | Binary decoder | Interpreter |
|-|-|-|-|-|-|
| `arg.get` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `call-core` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s8.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s8.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s16.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s16.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s32.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s32.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s64.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `s64.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_s8` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_s16` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_s32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_s64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_s8` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_s16` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_s32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_s64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u8.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u8.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u16.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u16.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u32.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u32.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u64.from_i32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `u64.from_i64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_u8` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_u16` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_u32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i32.from_u64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_u8` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_u16` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_u32` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `i64.from_u64` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `memory-to-string` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `string-to-memory` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `call-adapter` | ❌ | ❌ | ❌ | ❌ | ❌ |
| `defer-call-core` | ❌ | ❌ | ❌ | ❌ | ❌ |
