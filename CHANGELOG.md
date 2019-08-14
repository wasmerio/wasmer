# Changelog

All PRs to the Wasmer repository must add to this file.

Blocks of changes will separated by version increments.

## **[Unreleased]**

Special thanks to @YaronWittenstein @penberg for their contributions.

- [#650](https://github.com/wasmerio/wasmer/issues/650) Implement `wasi::path_rename`, improve WASI FS public api, and allow open files to exist even when the underlying file is deleted
- [#643](https://github.com/wasmerio/wasmer/issues/643) Implement `wasi::path_symlink` and improve WASI FS public api IO error reporting
- [#608](https://github.com/wasmerio/wasmer/issues/608) Implement wasi syscalls `fd_allocate`, `fd_sync`, `fd_pread`, `path_link`, `path_filestat_set_times`; update WASI fs API in a WIP way; reduce coupling of WASI code to host filesystem; make debug messages from WASI more readable; improve rights-checking when calling syscalls; implement reference counting on inodes; misc bug fixes and improvements
- [#616](https://github.com/wasmerio/wasmer/issues/616) Create the import object separately from instance instantiation in `runtime-c-api`
- [#620](https://github.com/wasmerio/wasmer/issues/620) Replace one `throw()` with `noexcept` in llvm backend
- [#618](https://github.com/wasmerio/wasmer/issues/618) Implement `InternalEvent::Breakpoint` in the llvm backend to allow metering in llvm
- [#615](https://github.com/wasmerio/wasmer/issues/615) Eliminate `FunctionEnvironment` construction in `feed_event()` speeding up to 70% of compilation in clif
- [#609](https://github.com/wasmerio/wasmer/issues/609) Update dependencies
- [#602](https://github.com/wasmerio/wasmer/issues/602) C api extract instance context from instance
- [#590](https://github.com/wasmerio/wasmer/issues/590) Error visibility changes in wasmer-c-api
- [#589](https://github.com/wasmerio/wasmer/issues/589) Make `wasmer_byte_array` fields `public` in wasmer-c-api

## 0.6.0 - 2019-07-31
- [#603](https://github.com/wasmerio/wasmer/pull/603) Update Wapm-cli, bump version numbers
- [#595](https://github.com/wasmerio/wasmer/pull/595) Add unstable public API for interfacing with the WASI file system in plugin-like usecases
- [#598](https://github.com/wasmerio/wasmer/pull/598) LLVM Backend is now supported in Windows
- [#599](https://github.com/wasmerio/wasmer/pull/599) Fix llvm backend failures in fat spec tests and simd_binaryen spec test.
- [#579](https://github.com/wasmerio/wasmer/pull/579) Fix bug in caching with LLVM and Singlepass backends.
  Add `default-backend-singlepass`, `default-backend-llvm`, and `default-backend-cranelift` features to `wasmer-runtime`
  to control the `default_compiler()` function (this is a breaking change).  Add `compiler_for_backend` function in `wasmer-runtime`
- [#561](https://github.com/wasmerio/wasmer/pull/561) Call the `data_finalizer` field on the `Ctx`
- [#576](https://github.com/wasmerio/wasmer/pull/576) fix `Drop` of uninit `Ctx`
- [#542](https://github.com/wasmerio/wasmer/pull/542) Add SIMD support to Wasmer (LLVM backend only)
  - Updates LLVM to version 8.0

## 0.5.7 - 2019-07-23
- [#575](https://github.com/wasmerio/wasmer/pull/575) Prepare for release; update wapm to 0.3.6
- [#555](https://github.com/wasmerio/wasmer/pull/555) WASI filesystem rewrite.  Major improvements
  - adds virtual root showing all preopened directories
  - improved sandboxing and code-reuse
  - symlinks work in a lot more situations
  - many misc. improvements to most syscalls touching the filesystem

## 0.5.6 - 2019-07-16
- [#565](https://github.com/wasmerio/wasmer/pull/565) Update wapm and bump version to 0.5.6
- [#563](https://github.com/wasmerio/wasmer/pull/563) Improve wasi testing infrastructure
  - fixes arg parsing from comments & fixes the mapdir test to have the native code doing the same thing as the WASI code
  - makes wasitests-generate output stdout/stderr by default & adds function to print stdout and stderr for a command if it fails
  - compiles wasm with size optimizations & strips generated wasm with wasm-strip
- [#554](https://github.com/wasmerio/wasmer/pull/554) Finish implementation of `wasi::fd_seek`, fix bug in filestat
- [#550](https://github.com/wasmerio/wasmer/pull/550) Fix singlepass compilation error with `imul` instruction


## 0.5.5 - 2019-07-10
- [#541](https://github.com/wasmerio/wasmer/pull/541) Fix dependency graph by making separate test crates; ABI implementations should not depend on compilers. Add Cranelift fork as git submodule of clif-backend
- [#537](https://github.com/wasmerio/wasmer/pull/537) Add hidden flag (`--cache-key`) to use prehashed key into the compiled wasm cache and change compiler backend-specific caching to use directories
- [#536](https://github.com/wasmerio/wasmer/pull/536) ~Update cache to use compiler backend name in cache key~

## 0.5.4 - 2019-07-06
- [#529](https://github.com/wasmerio/wasmer/pull/529) Updates the Wasm Interface library, which is used by wapm, with bug fixes and error message improvements

## 0.5.3 - 2019-07-03
- [#523](https://github.com/wasmerio/wasmer/pull/523) Update wapm version to fix bug related to signed packages in the global namespace and locally-stored public keys

## 0.5.2 - 2019-07-02
- [#516](https://github.com/wasmerio/wasmer/pull/516) Add workaround for singlepass miscompilation on GetLocal
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
