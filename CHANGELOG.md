# Changelog

All PRs to the Wasmer repository must add to this file.

Blocks of changes will separated by version increments.

## **[Unreleased]**
## 0.5.3
- [#523](https://github.com/wasmerio/wasmer/pull/523) Update wapm version to fix bug related to signed packages in the global namespace and locally-stored public keys

## 0.5.2 - 2019-07-02
- [#521](https://github.com/wasmerio/wasmer/pull/521) Update Wapm-cli, bump version numbers
- [#518](https://github.com/wasmerio/wasmer/pull/518) Update Cranelift and WasmParser
- [#514](https://github.com/wasmerio/wasmer/pull/514) [#519](https://github.com/wasmerio/wasmer/pull/519) Improved Emscripten network related calls, added a null check to `WasmPtr`
- [#515](https://github.com/wasmerio/wasmer/pull/515) Improved Emscripten dyncalls
- [#513](https://github.com/wasmerio/wasmer/pull/513) Fix emscripten lseek implementation.
- [#510](https://github.com/wasmerio/wasmer/pull/510) Simplify construction of floating point constants in LLVM backend. Fix LLVM assertion failure due to definition of %ctx.

## 0.5.1 - 2019-06-24
- [#508](https://github.com/wasmerio/wasmer/pull/508) Update wapm version, includes bug fixes

## 0.5.0 - 2019-06-17

- [#471](https://github.com/wasmerio/wasmer/pull/471) Added missing functions to run Python. Improved Emscripten bindings
- [#494](https://github.com/wasmerio/wasmer/pull/494) Remove deprecated type aliases from libc in the runtime C API
- [#493](https://github.com/wasmerio/wasmer/pull/493) `wasmer_module_instantiate` has better error messages in the runtime C API
- [#474](https://github.com/wasmerio/wasmer/pull/474) Set the install name of the dylib to `@rpath`
- [#490](https://github.com/wasmerio/wasmer/pull/490) Add MiddlewareChain and StreamingCompiler to runtime
- [#487](https://github.com/wasmerio/wasmer/pull/487) Fix stack offset check in singlepass backend 
- [#450](https://github.com/wasmerio/wasmer/pull/450) Added Metering
- [#481](https://github.com/wasmerio/wasmer/pull/481) Added context trampoline into runtime
- [#484](https://github.com/wasmerio/wasmer/pull/484) Fix bugs in emscripten socket syscalls
- [#476](https://github.com/wasmerio/wasmer/pull/476) Fix bug with wasi::environ_get, fix off by one error in wasi::environ_sizes_get
- [#470](https://github.com/wasmerio/wasmer/pull/470) Add mapdir support to Emscripten, implement getdents for Unix
- [#467](https://github.com/wasmerio/wasmer/pull/467) `wasmer_instantiate` returns better error messages in the runtime C API
- [#463](https://github.com/wasmerio/wasmer/pull/463) Fix bug in WASI path_open allowing one level above preopened dir to be accessed
- [#461](https://github.com/wasmerio/wasmer/pull/461) Prevent passing negative lengths in various places in the runtime C API
- [#459](https://github.com/wasmerio/wasmer/pull/459) Add monotonic and real time clocks for wasi on windows
- [#447](https://github.com/wasmerio/wasmer/pull/447) Add trace macro (`--features trace`) for more verbose debug statements
- [#451](https://github.com/wasmerio/wasmer/pull/451) Add `--mapdir=src:dest` flag to rename host directories in the guest context
- [#457](https://github.com/wasmerio/wasmer/pull/457) Implement file metadata for WASI, fix bugs in WASI clock code for Unix platforms

## 0.4.2 - 2019-05-16

- [#416](https://github.com/wasmerio/wasmer/pull/416) Remote code loading framework
- [#449](https://github.com/wasmerio/wasmer/pull/449) Fix bugs: opening host files in filestat and opening with write permissions unconditionally in path_open
- [#442](https://github.com/wasmerio/wasmer/pull/442) Misc. WASI FS fixes and implement readdir
- [#440](https://github.com/wasmerio/wasmer/pull/440) Fix type mismatch between `wasmer_instance_call` and `wasmer_export_func_*_arity` functions in the runtime C API.
- [#269](https://github.com/wasmerio/wasmer/pull/269) Add better runtime docs
- [#432](https://github.com/wasmerio/wasmer/pull/432) Fix returned value of `wasmer_last_error_message` in the runtime C API
- [#429](https://github.com/wasmerio/wasmer/pull/429) Get wasi::path_filestat_get working for some programs; misc. minor WASI FS improvements
- [#413](https://github.com/wasmerio/wasmer/pull/413) Update LLVM backend to use new parser codegen traits

## 0.4.1 - 2019-05-06

- [#426](https://github.com/wasmerio/wasmer/pull/426) Update wapm-cli submodule, bump version to 0.4.1
- [#422](https://github.com/wasmerio/wasmer/pull/422) Improved Emscripten functions to run optipng and pngquant compiled to wasm
- [#409](https://github.com/wasmerio/wasmer/pull/409) Improved Emscripten functions to run JavascriptCore compiled to wasm
- [#399](https://github.com/wasmerio/wasmer/pull/399) Add example of using a plugin extended from WASI
- [#397](https://github.com/wasmerio/wasmer/pull/397) Fix WASI fs abstraction to work on Windows
- [#390](https://github.com/wasmerio/wasmer/pull/390) Pin released wapm version and add it as a git submodule
- [#408](https://github.com/wasmerio/wasmer/pull/408) Add images to windows installer and update installer to add wapm bin directory to path

## 0.4.0 - 2019-04-23

- [#383](https://github.com/wasmerio/wasmer/pull/383) Hook up wasi exit code to wasmer cli.
- [#382](https://github.com/wasmerio/wasmer/pull/382) Improve error message on `--backend` flag to only suggest currently enabled backends
- [#381](https://github.com/wasmerio/wasmer/pull/381) Allow retrieving propagated user errors.
- [#379](https://github.com/wasmerio/wasmer/pull/379) Fix small return types from imported functions.
- [#371](https://github.com/wasmerio/wasmer/pull/371) Add more Debug impl for WASI types
- [#368](https://github.com/wasmerio/wasmer/pull/368) Fix issue with write buffering
- [#343](https://github.com/wasmerio/wasmer/pull/343) Implement preopened files for WASI and fix aligment issue when accessing WASI memory
- [#367](https://github.com/wasmerio/wasmer/pull/367) Add caching support to the LLVM backend.
- [#366](https://github.com/wasmerio/wasmer/pull/366) Remove `UserTrapper` trait to fix [#365](https://github.com/wasmerio/wasmer/issues/365).
- [#348](https://github.com/wasmerio/wasmer/pull/348) Refactor internal runtime ↔️ backend abstraction.
- [#355](https://github.com/wasmerio/wasmer/pull/355) Misc changes to `Cargo.toml`s for publishing
- [#352](https://github.com/wasmerio/wasmer/pull/352) Bump version numbers to 0.3.0
- [#351](https://github.com/wasmerio/wasmer/pull/351) Add hidden option to specify wasm program name (can be used to improve error messages)
- [#350](https://github.com/wasmerio/wasmer/pull/350) Enforce that CHANGELOG.md is updated through CI.
- [#349](https://github.com/wasmerio/wasmer/pull/349) Add [CHANGELOG.md](https://github.com/wasmerio/wasmer/blob/master/CHANGELOG.md).

## 0.3.0 - 2019-04-12

- [#276](https://github.com/wasmerio/wasmer/pull/276) [#288](https://github.com/wasmerio/wasmer/pull/288) [#344](https://github.com/wasmerio/wasmer/pull/344) Use new singlepass backend (with the `--backend=singlepass` when running Wasmer)
- [#338](https://github.com/wasmerio/wasmer/pull/338) Actually catch traps/panics/etc when using a typed func.
- [#325](https://github.com/wasmerio/wasmer/pull/325) Fixed func_index in debug mode
- [#323](https://github.com/wasmerio/wasmer/pull/323) Add validate subcommand to validate Wasm files
- [#321](https://github.com/wasmerio/wasmer/pull/321) Upgrade to Cranelift 0.3.0
- [#319](https://github.com/wasmerio/wasmer/pull/319) Add Export and GlobalDescriptor to Runtime API
- [#310](https://github.com/wasmerio/wasmer/pull/310) Cleanup warnings
- [#299](https://github.com/wasmerio/wasmer/pull/299) [#300](https://github.com/wasmerio/wasmer/pull/300) [#301](https://github.com/wasmerio/wasmer/pull/301) [#303](https://github.com/wasmerio/wasmer/pull/303) [#304](https://github.com/wasmerio/wasmer/pull/304) [#305](https://github.com/wasmerio/wasmer/pull/305) [#306](https://github.com/wasmerio/wasmer/pull/306) [#307](https://github.com/wasmerio/wasmer/pull/307) Add support for WASI 🎉
- [#286](https://github.com/wasmerio/wasmer/pull/286) Add extend to imports
- [#278](https://github.com/wasmerio/wasmer/pull/278) Add versioning to cache
- [#250](https://github.com/wasmerio/wasmer/pull/250) Setup bors
