# Changelog

All PRs to the Wasmer repository must add to this file.

Blocks of changes will separated by version increments.

## **[Unreleased]**

## 0.8.0 - 2019-10-02

Special thanks to @jdanford for their contributions!

- [#850](https://github.com/wasmerio/wasmer/pull/850) New `WasiStateBuilder` API. small, add misc. breaking changes to existing API (for example, changing the preopen dirs arg on `wasi::generate_import_object` from `Vec<String>` to `Vec<Pathbuf>`)
- [#852](https://github.com/wasmerio/wasmer/pull/852) Make minor grammar/capitalization fixes to README.md
- [#841](https://github.com/wasmerio/wasmer/pull/841) Slightly improve rustdoc documentation and small updates to outdated info in readme files
- [#836](https://github.com/wasmerio/wasmer/pull/836) Update Cranelift fork version to `0.44.0`
- [#839](https://github.com/wasmerio/wasmer/pull/839) Change supported version to stable Rust 1.37+
- [#834](https://github.com/wasmerio/wasmer/pull/834) Fix panic when unwraping `wasmer` arguments
- [#835](https://github.com/wasmerio/wasmer/pull/835) Add parallel execution example (independent instances created from the same `ImportObject` and `Module` run with rayon)
- [#834](https://github.com/wasmerio/wasmer/pull/834) Fix panic when parsing numerical arguments for no-ABI targets run with the wasmer binary
- [#833](https://github.com/wasmerio/wasmer/pull/833) Add doc example of using ImportObject's new `maybe_with_namespace` method
- [#832](https://github.com/wasmerio/wasmer/pull/832) Delete unused runtime ABI
- [#809](https://github.com/wasmerio/wasmer/pull/809) Fix bugs leading to panics in `LocalBacking`.
- [#831](https://github.com/wasmerio/wasmer/pull/831) Add support for atomic operations, excluding wait and notify, to singlepass.
- [#822](https://github.com/wasmerio/wasmer/pull/822) Update Cranelift fork version to `0.43.1`
- [#829](https://github.com/wasmerio/wasmer/pull/829) Fix deps on `make bench-*` commands; benchmarks don't compile other backends now
- [#807](https://github.com/wasmerio/wasmer/pull/807) Implement Send for `Instance`, breaking change on `ImportObject`, remove method `get_namespace` replaced with `with_namespace` and `maybe_with_namespace`
- [#817](https://github.com/wasmerio/wasmer/pull/817) Add document for tracking features across backends and language integrations, [docs/feature_matrix.md]
- [#823](https://github.com/wasmerio/wasmer/issues/823) Improved Emscripten / WASI integration
- [#821](https://github.com/wasmerio/wasmer/issues/821) Remove patch version on most deps Cargo manifests.  This gives Wasmer library users more control over which versions of the deps they use.
- [#820](https://github.com/wasmerio/wasmer/issues/820) Remove null-pointer checks in `WasmPtr` from runtime-core, re-add them in Emscripten
- [#803](https://github.com/wasmerio/wasmer/issues/803) Add method to `Ctx` to invoke functions by their `TableIndex`
- [#790](https://github.com/wasmerio/wasmer/pull/790) Fix flaky test failure with LLVM, switch to large code model.
- [#788](https://github.com/wasmerio/wasmer/pull/788) Use union merge on the changelog file.
- [#785](https://github.com/wasmerio/wasmer/pull/785) Include Apache license file for spectests.
- [#786](https://github.com/wasmerio/wasmer/pull/786) In the LLVM backend, lower atomic wasm operations to atomic machine instructions.
- [#784](https://github.com/wasmerio/wasmer/pull/784) Fix help string for wasmer run.

## 0.7.0 - 2019-09-12

Special thanks to @YaronWittenstein @penberg for their contributions.

- [#776](https://github.com/wasmerio/wasmer/issues/776) Allow WASI preopened fds to be closed
- [#774](https://github.com/wasmerio/wasmer/issues/774) Add more methods to the `WasiFile` trait
- [#772](https://github.com/wasmerio/wasmer/issues/772) [#770](https://github.com/wasmerio/wasmer/issues/770) Handle more internal failures by passing back errors
- [#756](https://github.com/wasmerio/wasmer/issues/756) Allow NULL parameter and 0 arity in `wasmer_export_func_call` C API
- [#747](https://github.com/wasmerio/wasmer/issues/747) Return error instead of panicking on traps when using the Wasmer binary
- [#741](https://github.com/wasmerio/wasmer/issues/741) Add validate Wasm fuzz target
- [#733](https://github.com/wasmerio/wasmer/issues/733) Remove dependency on compiler backends for `middleware-common`
- [#732](https://github.com/wasmerio/wasmer/issues/732) [#731](https://github.com/wasmerio/wasmer/issues/731) WASI bug fixes and improvements
- [#726](https://github.com/wasmerio/wasmer/issues/726) Add serialization and deserialization for Wasi State
- [#716](https://github.com/wasmerio/wasmer/issues/716) Improve portability of install script
- [#714](https://github.com/wasmerio/wasmer/issues/714) Add Code of Conduct
- [#708](https://github.com/wasmerio/wasmer/issues/708) Remove unconditional dependency on Cranelift in the C API
- [#703](https://github.com/wasmerio/wasmer/issues/703) Fix compilation on AArch64 Linux
- [#702](https://github.com/wasmerio/wasmer/issues/702) Add SharedMemory to Wasmer. Add `--enable-threads` flag, add partial implementation of atomics to LLVM backend.
- [#698](https://github.com/wasmerio/wasmer/issues/698) [#690](https://github.com/wasmerio/wasmer/issues/690) [#687](https://github.com/wasmerio/wasmer/issues/690) Fix panics in Emscripten
- [#689](https://github.com/wasmerio/wasmer/issues/689) Replace `wasmer_runtime_code::memory::Atomic` with `std::sync::atomic` atomics, changing its interface
- [#680](https://github.com/wasmerio/wasmer/issues/680) [#673](https://github.com/wasmerio/wasmer/issues/673) [#669](https://github.com/wasmerio/wasmer/issues/669) [#660](https://github.com/wasmerio/wasmer/issues/660) [#659](https://github.com/wasmerio/wasmer/issues/659) Misc. runtime and singlepass fixes
- [#677](https://github.com/wasmerio/wasmer/issues/677) [#675](https://github.com/wasmerio/wasmer/issues/675) [#674](https://github.com/wasmerio/wasmer/issues/674) LLVM backend fixes and improvements
- [#671](https://github.com/wasmerio/wasmer/issues/671) Implement fs polling in `wasi::poll_oneoff` for Unix-like platforms
- [#656](https://github.com/wasmerio/wasmer/issues/656) Move CI to Azure Pipelines
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
