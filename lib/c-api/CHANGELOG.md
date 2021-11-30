# C API Changelog

*The format is based on [Keep a Changelog].*

[Keep a Changelog]: http://keepachangelog.com/en/1.0.0/

Looking for changes to the Wasmer CLI and the Rust API? See our [Primary Changelog](../../CHANGELOG.md)

## **[Unreleased]**

## 2.1.0 - 2021/11/30

### Added
- [#2521](https://github.com/wasmerio/wasmer/pull/2521) Create `OrderedResolver` from a parallel iterator, which improves performances of `wasm_new_instance` when a large set of imports is given.
- [#2449](https://github.com/wasmerio/wasmer/pull/2449) Configure `soname`, `install_name`, `out-implib`, etc.

### Changed
- [#2478](https://github.com/wasmerio/wasmer/pull/2478) Rename `traps` input to `wasm_instance_new()` to `trap`.

### Fixed
- [#2485](https://github.com/wasmerio/wasmer/pull/2493) Document wasm_limits_t’s members publicly.
- [#2444](https://github.com/wasmerio/wasmer/pull/2444) Trap's messages are always null terminated.
- [#2683](https://github.com/wasmerio/wasmer/pull/2683) Fix memory leaks in the C API.

## 2.0.0 - 2020/06/16

## 2.0.0-rc1 - 2020/06/02

### Added
- [#2346](https://github.com/wasmerio/wasmer/pull/2346) Add missing `wasm_func_copy` function.
- [#2208](https://github.com/wasmerio/wasmer/pull/2208) Add a new CHANGELOG.md specific to our C API to make it easier for users primarily consuming our C API to keep up to date with changes that affect them.
- [#2103](https://github.com/wasmerio/wasmer/pull/2103) Add middleware (incl. metering) API.
- [#2153](https://github.com/wasmerio/wasmer/pull/2153) Add a `wasmer_features_t` unstable C API to define features for the engine and the compiler in the Wasm C API.
- [#2118](https://github.com/wasmerio/wasmer/pull/2118) Add an unstable non-standard C API to query available engines and compilers.

### Changed
- [#2375](https://github.com/wasmerio/wasmer/pull/2375) Rename `wasmer_wasm.h` to `wasmer.h` (old behavior still continues to work).
- [#2370](https://github.com/wasmerio/wasmer/pull/2370) Remove the deprecated C API.

### Fixed
- [#2208](https://github.com/wasmerio/wasmer/pull/2208) Fix ownership of `wasm_extern_as_func`, `wasm_extern_as_memory`, `wasm_extern_as_table`, `wasm_extern_as_global`, `wasm_func_as_extern`, `wasm_memory_as_extern`, `wasm_table_as_extern`, and `wasm_global_as_extern`. These functions no longer allocate memory and thus their results should not be freed. This is a breaking change to align more closely with the Wasm C API's stated ownership.
- [#2210](https://github.com/wasmerio/wasmer/pull/2210) Fix a memory leak in the strings used to identify imports and exports coming from user code.
- [#2117](https://github.com/wasmerio/wasmer/pull/2117) Formalize API prefixes. Only unstable functions have been renamed.
- [#2097](https://github.com/wasmerio/wasmer/pull/2097) Fix how string's length is computed in `wasm_cpu_features_add`.

## Changes before 2020-04-06

See the [Primary Changelog](../../CHANGELOG.md).
