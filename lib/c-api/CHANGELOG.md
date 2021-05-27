# C API Changelog

*The format is based on [Keep a Changelog].*

[Keep a Changelog]: http://keepachangelog.com/en/1.0.0/

Looking for changes to the Wasmer CLI and the Rust API? See our [Primary Changelog](../../CHANGELOG.md)


## **[Unreleased]**

### Added
- [#2346](https://github.com/wasmerio/wasmer/pull/2346) Add missing `wasm_func_copy` function.
- [#2208](https://github.com/wasmerio/wasmer/pull/2208) Add a new CHANGELOG.md specific to our C API to make it easier for users primarily consuming our C API to keep up to date with changes that affect them.

### Changed

### Fixed
- [#2208](https://github.com/wasmerio/wasmer/pull/2208) Fix ownership in Wasm C API of `wasm_extern_as_func`, `wasm_extern_as_memory`, `wasm_extern_as_table`, `wasm_extern_as_global`, `wasm_func_as_extern`, `wasm_memory_as_extern`, `wasm_table_as_extern`, and `wasm_global_as_extern`. These functions no longer allocate memory and thus their results should not be freed. This is a breaking change to align more closely with the Wasm C API's stated ownership.

## Changes before 2020-04-06

See the [Primary Changelog](../../CHANGELOG.md).
