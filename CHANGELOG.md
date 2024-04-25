# Changelog

*The format is based on [Keep a Changelog].*

[Keep a Changelog]: http://keepachangelog.com/en/1.0.0/

Looking for changes that affect our C API? See the [C API Changelog](lib/c-api/CHANGELOG.md).


## **Unreleased**

## 4.3.0-alpha.1 - 25/04/2024

This release introduces support for publishing unnamed packages. It also fixes an issue with code generation when using Singlepass and contains fixes to WASIX.

## Added

  - [#4532](https://github.com/wasmerio/wasmer/pull/4532) Unnamed packages
  - [#4548](https://github.com/wasmerio/wasmer/pull/4548) Added a fix so that closed sockets do not cause errors
  - [#4560](https://github.com/wasmerio/wasmer/pull/4560) chore(backend-api): Add hostname to AppAlias type
  - [#4543](https://github.com/wasmerio/wasmer/pull/4543) build: Add Wasmer CLI package definition to Nix flake

## Changed

  - [#4582](https://github.com/wasmerio/wasmer/pull/4582) feat: wasmer-config
  - [#4359](https://github.com/wasmerio/wasmer/pull/4359) Use nanoseconds in `filestat` for directories
  - [#4557](https://github.com/wasmerio/wasmer/pull/4557) Safely handle offset in fd_seek
  - [#4555](https://github.com/wasmerio/wasmer/pull/4555) Fd validity and basic bound checks on `fd_advise`
  - [#4538](https://github.com/wasmerio/wasmer/pull/4538) deps: Switch from unmaintained term_size to terminal_size
  - [#4563](https://github.com/wasmerio/wasmer/pull/4563) chore: Remove accidentally committed wasm file
  - [#4561](https://github.com/wasmerio/wasmer/pull/4561) chore: Make wasmer_wasix::os::console submodule public (again)
  - [#4562](https://github.com/wasmerio/wasmer/pull/4562) chore: remove repetitive words
  - [#4552](https://github.com/wasmerio/wasmer/pull/4552) Module cache optimization round2
  - [#4546](https://github.com/wasmerio/wasmer/pull/4546) Expose `store::StoreObjects`

## Fixed

  - [#4559](https://github.com/wasmerio/wasmer/pull/4559) Fix ImpossibleRelocation panics in singlepass/aarch64
  - [#4514](https://github.com/wasmerio/wasmer/pull/4514) Fix for snapshots in certain use cases
  - [#4590](https://github.com/wasmerio/wasmer/pull/4590) Fix fd_seek underflow



## 4.2.8 - 05/04/2024

This release improves journal support and improves the performance of the singlepass backend.
Also contains fixes to the Edge CLI.

## Added

  - [#4510](https://github.com/wasmerio/wasmer/pull/4510) Added support for creating log file journals directly from buffers
  - [#4506](https://github.com/wasmerio/wasmer/pull/4506) feat: add wasmer-argus
  - [#4508](https://github.com/wasmerio/wasmer/pull/4508) Upgrade edge-{schema,util} crates + add some helper methdos

## Changed

  - [#4541](https://github.com/wasmerio/wasmer/pull/4541) Removed some dead code
  - [#4539](https://github.com/wasmerio/wasmer/pull/4539) deps: Upgrade h2 due to RUSTSEC advisory
  - [#4527](https://github.com/wasmerio/wasmer/pull/4527) allow owner field in app.yaml
  - [#4526](https://github.com/wasmerio/wasmer/pull/4526) feat(singlepass): use SIMD insts for popcount
  - [#4507](https://github.com/wasmerio/wasmer/pull/4507) deps: Upgrade edge-schema to 0.0.3
  - [#4462](https://github.com/wasmerio/wasmer/pull/4462) DProxy

## Fixed

  - [#4542](https://github.com/wasmerio/wasmer/pull/4542) Various fixes detected in the build
  - [#4537](https://github.com/wasmerio/wasmer/pull/4537) Fix owner issues with app create
  - [#4535](https://github.com/wasmerio/wasmer/pull/4535) fix(cli): Fix Edge WinterJS template
  - [#4525](https://github.com/wasmerio/wasmer/pull/4525) Fix bug with `app deploy`: app URL is stale
  - [#4520](https://github.com/wasmerio/wasmer/pull/4520) Fix singlepass panic



## 4.2.7 - 19/03/2024

This release adds the `wasmer domain` command for DNS records management, and also includes an important fix to the `stack_restore` WASIX syscall (used by the `longjmp` function).

## Added

  - [#4478](https://github.com/wasmerio/wasmer/pull/4478) chore(backend-api): Add size to PackageDistribution

## Changed

  - [#4492](https://github.com/wasmerio/wasmer/pull/4492) No longer restoring the thread local memory when we longjmp
  - [#4487](https://github.com/wasmerio/wasmer/pull/4487) Manage DNS records
  - [#4220](https://github.com/wasmerio/wasmer/pull/4220) Ability to detect a tainted instance
  - [#4455](https://github.com/wasmerio/wasmer/pull/4455) Implemented an exponential CPU backoff that kicks in when a run token is not held
  - [#4470](https://github.com/wasmerio/wasmer/pull/4470) chore: Completely remove wasix_http_client

## Fixed

  - [#4490](https://github.com/wasmerio/wasmer/pull/4490) Fix for a panic in the sock_recv when a file handle is missing
  - [#4335](https://github.com/wasmerio/wasmer/pull/4335) Fixed an issue where the package loader was blocking the tokio runtime
  - [#4473](https://github.com/wasmerio/wasmer/pull/4473) fix: fix feature = "cargo-clippy" deprecation



## 4.2.6 - 03/03/2024

This release includes a number of DX improvements for the Wasmer CLI, as well as fixes to WASI and its filesystem implementation.

## Added

  - [#4459](https://github.com/wasmerio/wasmer/pull/4459) feat(backend-api): Add download_url to PackageDistribution
  - [#4460](https://github.com/wasmerio/wasmer/pull/4460) fix(cli): Add missing output in "app get" command and fix output of "app info"
  - [#4448](https://github.com/wasmerio/wasmer/pull/4448) Add wasi-fyi tests and run them in CI
  - [#4454](https://github.com/wasmerio/wasmer/pull/4454) add pagination to app list queries
  - [#4443](https://github.com/wasmerio/wasmer/pull/4443) feat(backend-api): Add get_app_by_id_opt
  - [#4385](https://github.com/wasmerio/wasmer/pull/4385) feat(api): Add SharedMemoryHandle
  - [#3517](https://github.com/wasmerio/wasmer/pull/3517) Add HTTP example which does dynamic allocation

## Changed

  - [#4472](https://github.com/wasmerio/wasmer/pull/4472) feat(cli): "wasmer deploy": return app version as json if --json specified
  - [#4471](https://github.com/wasmerio/wasmer/pull/4471) chore: Remove wasi-experimental-io-devices
  - [#4466](https://github.com/wasmerio/wasmer/pull/4466) Clean up wasmer-wasix-types and remove wasix-http-client
  - [#4458](https://github.com/wasmerio/wasmer/pull/4458) tests: Disable flaky network tests on MUSL
  - [#4440](https://github.com/wasmerio/wasmer/pull/4440) cli: allow filtering by log stream
  - [#4435](https://github.com/wasmerio/wasmer/pull/4435) Use M1 runner for building & testing m1
  - [#4434](https://github.com/wasmerio/wasmer/pull/4434) Update Rust toolchain to 1.73
  - [#4428](https://github.com/wasmerio/wasmer/pull/4428) Try to build the package before publishing it
  - [#4441](https://github.com/wasmerio/wasmer/pull/4441) increase pagination size for app logs
  - [#4429](https://github.com/wasmerio/wasmer/pull/4429) Return Notsup for absolute symlinks instead of panicking
  - [#4427](https://github.com/wasmerio/wasmer/pull/4427) Migrate deprecated usage
  - [#4426](https://github.com/wasmerio/wasmer/pull/4426) deps: Unify wasmparser usage to a single version
  - [#4292](https://github.com/wasmerio/wasmer/pull/4292) deps: Upgrade wasmparser
  - [#4398](https://github.com/wasmerio/wasmer/pull/4398) Make public API consistent
  - [#4407](https://github.com/wasmerio/wasmer/pull/4407) CLI: Big CLI cleanup + merge Edge CLI
  - [#4395](https://github.com/wasmerio/wasmer/pull/4395) chore(wasix): Increase log level for WapmSource
  - [#4403](https://github.com/wasmerio/wasmer/pull/4403) Allow users to provide custom host functions to `WasiRunner` and `WasiEnvBuilder`
  - [#4359](https://github.com/wasmerio/wasmer/pull/4359) Use nanoseconds in `filestat` for directories
  - [#4100](https://github.com/wasmerio/wasmer/pull/4100) Tweak logging annotations to simplify performance troubleshooting
  - [#4175](https://github.com/wasmerio/wasmer/pull/4175) Update dependencies and remove unused dependencies
  - [#4302](https://github.com/wasmerio/wasmer/pull/4302) Allow `WasiRunner` to mount `FileSystem` instances
  - [#4394](https://github.com/wasmerio/wasmer/pull/4394) ignore append when truncate is set
  - [#4263](https://github.com/wasmerio/wasmer/pull/4263) WASI journal and stateful persistence
  - [#4389](https://github.com/wasmerio/wasmer/pull/4389) remove memory footprint computation
  - [#4352](https://github.com/wasmerio/wasmer/pull/4352) Wait for webc, bindings, exe generation during package-publish with `--wait` flag
  - [#4388](https://github.com/wasmerio/wasmer/pull/4388) Temporarily exclude tokio from docs builds
  - [#4345](https://github.com/wasmerio/wasmer/pull/4345) Asyncify filesystem cache

## Fixed

  - [#4467](https://github.com/wasmerio/wasmer/pull/4467) fix(cli): Fix auto-package version bumping in 'wasmer deploy'
  - [#4452](https://github.com/wasmerio/wasmer/pull/4452) CLI: Fix logic error in indexing in app logs command
  - [#4444](https://github.com/wasmerio/wasmer/pull/4444) Fix note about unaligned references in `WasmRef` docs
  - [#4338](https://github.com/wasmerio/wasmer/pull/4338) [SINGLEPASS]Fix bug #4092 which was due to resource leak while doing …
  - [#4430](https://github.com/wasmerio/wasmer/pull/4430) deps: Fix accidental hyper 1.0 upgrade + remove some duplicate dependencies
  - [#4322](https://github.com/wasmerio/wasmer/pull/4322) fix: Prevent panicking in VirtualTaskManagerExt::spawn_and_block_on
  - [#4411](https://github.com/wasmerio/wasmer/pull/4411) chore: Fix clippy lints for headless CLI build
  - [#4410](https://github.com/wasmerio/wasmer/pull/4410) fix(cli): publish: Fix panic + Make waiting for build results more modular
  - [#4402](https://github.com/wasmerio/wasmer/pull/4402) Fixed some compilation errors introduced by #4204 that assume `tokio::task::block_in_place()` always exists
  - [#3448](https://github.com/wasmerio/wasmer/pull/3448) Fix make install-capi-lib
  - [#4393](https://github.com/wasmerio/wasmer/pull/4393) fix(api): Provide stub impls for new LinearMemory methods
  - [#4391](https://github.com/wasmerio/wasmer/pull/4391) Fixed an issue where the caching compiled modules were not saving properly
  - [#4386](https://github.com/wasmerio/wasmer/pull/4386) chore: Fix clippy lints in API crate
  - [#4387](https://github.com/wasmerio/wasmer/pull/4387) fix: Don't log the `FmtSpan::ENTER` event because it generates unnecessary logs



## 4.2.5 - 23/12/2023

This release includes substantial improvements to Wasmer's startup time when loading cached modules. Also includes a couple of filesystem-related fixes and updates the Edge application templates.

## Added

  - [#4370](https://github.com/wasmerio/wasmer/pull/4370) feat(wasix): Add optional chunk timeout to ReqwestHttpClient
  - [#4369](https://github.com/wasmerio/wasmer/pull/4369) WASIX: Add QueryError::Timeout
  - [#4336](https://github.com/wasmerio/wasmer/pull/4336) fix(docs): add cpp in Chinese

## Changed

  - [#4372](https://github.com/wasmerio/wasmer/pull/4372) use faster hash algorithm when loading and saving a module
  - [#4367](https://github.com/wasmerio/wasmer/pull/4367) deps: Bump edge cli to 0.1.4
  - [#4366](https://github.com/wasmerio/wasmer/pull/4366) Always use the download URL provided by the Wasmer registry instead of going through the frontend redirect
  - [#4350](https://github.com/wasmerio/wasmer/pull/4350) Optimize sleep_now for TokioTaskManager
  - [#4351](https://github.com/wasmerio/wasmer/pull/4351) Remove the wasmer-web crate and all tests
  - [#4229](https://github.com/wasmerio/wasmer/pull/4229) Use `cargo nextest` when running tests in CI
  - [#4339](https://github.com/wasmerio/wasmer/pull/4339) deps: Bump edge-cli version

## Fixed

  - [#4368](https://github.com/wasmerio/wasmer/pull/4368) fix(wasix): http client: use reasonable default connect timeout
  - [#4365](https://github.com/wasmerio/wasmer/pull/4365) lib/api/src/js/store.rs: fix typo
  - [#4356](https://github.com/wasmerio/wasmer/pull/4356) Fixed a subtract-with-overflow in `StaticFile::poll_read_ready()`
  - [#4355](https://github.com/wasmerio/wasmer/pull/4355) Fix the `virtual_fs::StaticFile` implementation
  - [#4348](https://github.com/wasmerio/wasmer/pull/4348) fix sync



## 4.2.4 - 30/11/2023

This release allows publishing private packages and fixes the issue of the file system being accessible by WASI modules in the abscence of directory mapping. Also improves startup speed, and fixes multiple issues around the WASI filesystem, packages and apps.

## Added

  - [#4334](https://github.com/wasmerio/wasmer/pull/4334) Add `application/wasm` to list of accepted content-types for webcs
  - [#4328](https://github.com/wasmerio/wasmer/pull/4328) Add `--wait` and `--timeout` flags to `wamer publish`
  - [#4315](https://github.com/wasmerio/wasmer/pull/4315) Add TTY aware output to the wasmer package and wasmer container commands
  - [#4287](https://github.com/wasmerio/wasmer/pull/4287) feat(cli): Add package commands
  - [#4247](https://github.com/wasmerio/wasmer/pull/4247) Add support for publishing private packages
  - [#4291](https://github.com/wasmerio/wasmer/pull/4291) feat(cli): add pnpm support

## Changed

  - [#4333](https://github.com/wasmerio/wasmer/pull/4333) deps: Bump edge-cli
  - [#4332](https://github.com/wasmerio/wasmer/pull/4332) use rusty_pool instead of rayon
  - [#4321](https://github.com/wasmerio/wasmer/pull/4321) deps(cli): Upgrade Edge CLI
  - [#4326](https://github.com/wasmerio/wasmer/pull/4326) Always re-execute a registry query when cache lookups fail
  - [#4317](https://github.com/wasmerio/wasmer/pull/4317) Bump min enumset version to 1.1.0
  - [#4300](https://github.com/wasmerio/wasmer/pull/4300) Use authentication when running a package
  - [#4294](https://github.com/wasmerio/wasmer/pull/4294) Terminate after flushing file descriptors
  - [#4273](https://github.com/wasmerio/wasmer/pull/4273) Update memoffset to 0.9.0

## Fixed

  - [#4307](https://github.com/wasmerio/wasmer/pull/4307) Fix for the non-flushing of file descriptors and a nasty deadlock
  - [#4331](https://github.com/wasmerio/wasmer/pull/4331) Fix visibility validation to work when publishing a new package
  - [#4314](https://github.com/wasmerio/wasmer/pull/4314) fix(cli): Prevent temporary file issues in "package download"
  - [#4296](https://github.com/wasmerio/wasmer/pull/4296) fix: prevent potential UB by deriving repr C for union
  - [#4192](https://github.com/wasmerio/wasmer/pull/4192) More fixes to support Wasmer JS



## 4.2.3 - 26/10/2023

This new version fixes a bug in module bindings path.

## Added


## Changed


## Fixed

  - [#4272](https://github.com/wasmerio/wasmer/pull/4272) fix: Fix async runtime mismatch in virtual "wasmer run" command
  - [#4276](https://github.com/wasmerio/wasmer/pull/4276) fix: module path using stripping a prefix
  - [#4241](https://github.com/wasmerio/wasmer/pull/4241) Fix FreeBSD ucontext_t issues
  - [#4246](https://github.com/wasmerio/wasmer/pull/4246) Fix Cargo.lock for 4.2.2 release



## 4.2.2 - 04/10/2023
New wasmer version that utilizes the more recent LLVM15 to compile WASM modules. Also adds new application templates for Wasmer Edge.

## Added


## Changed

  - [#4239](https://github.com/wasmerio/wasmer/pull/4239) Wasmer Deploy CLI with js and py worker templates
  - [#4238](https://github.com/wasmerio/wasmer/pull/4238) Using hard coded package version number in test...
  - [#4206](https://github.com/wasmerio/wasmer/pull/4206) Remove StdioMode::Reserved variant
  - [#4234](https://github.com/wasmerio/wasmer/pull/4234) Migration to LLVM15
  - [#4236](https://github.com/wasmerio/wasmer/pull/4236) Updated tugstsenite

## Fixed




## 4.2.1 - 28/09/2023
New release of wasmer with improved WASIX compatibility. MSRV was bumped to 1.70 for this release. 

Now it is possible for packages to execute atoms from their dependencies, enabling e.g. python packages to not include the binary but use the one from the official python package.

## Added

  - [#4213](https://github.com/wasmerio/wasmer/pull/4213) feat(wasix): Add BuiltinPackageLoader::insert_cached
  - [#4202](https://github.com/wasmerio/wasmer/pull/4202) Add support for `JoinFlags::NON_BLOCKING` to `proc_join`

## Changed

  - [#4223](https://github.com/wasmerio/wasmer/pull/4223) Allow packages to have commands that use a dependency's atom
  - [#4224](https://github.com/wasmerio/wasmer/pull/4224) Use write instead of send for pipe, to accomodate socketpair
  - [#4221](https://github.com/wasmerio/wasmer/pull/4221) Bump the MSRV from 1.69 to 1.70
  - [#4214](https://github.com/wasmerio/wasmer/pull/4214) Rephrase the docstring for `--forward-host-env` flag on `wasmer run`
  - [#4215](https://github.com/wasmerio/wasmer/pull/4215) Ignore `fd_close(3)` to avoid breaking POSIX programs that blindly close all file descriptors
  - [#4204](https://github.com/wasmerio/wasmer/pull/4204) Stability improvements

## Fixed

  - [#4211](https://github.com/wasmerio/wasmer/pull/4211) Fixes for sockets
  - [#4193](https://github.com/wasmerio/wasmer/pull/4193) Fix sockets
  - [#4205](https://github.com/wasmerio/wasmer/pull/4205) Fix File System Merging Problems



## 4.2.0 - 05/09/2023
New release of wasmer, with a new 0-copy module deserialization for shorter startup time, some fixes to avoid misaligned pointer acces, and faster internal stack handling, among some other fixes.

## Added

  - [#4199](https://github.com/wasmerio/wasmer/pull/4199) Added some Socket filtype return for fdstat syscall
  - [#4186](https://github.com/wasmerio/wasmer/pull/4186) Add stdin/stdout/stderr streams to `WasiRunner` and only use async threading when requested

## Changed

  - [#4170](https://github.com/wasmerio/wasmer/pull/4170) deps: Bump Edge client CLI to 0.1.25
  - [#4179](https://github.com/wasmerio/wasmer/pull/4179) Faster compiles for debug by using release version of cranelift
  - [#4196](https://github.com/wasmerio/wasmer/pull/4196) Replace stack pool mutex with lock-free queue
  - [#4180](https://github.com/wasmerio/wasmer/pull/4180) NativeEngineExt::deserialize now returns Module
  - [#4190](https://github.com/wasmerio/wasmer/pull/4190) Early check that a cached artifact is compatible with current CPU Features
  - [#4167](https://github.com/wasmerio/wasmer/pull/4167) Make sure vmoffset are aligned to pointer size (for #4059)
  - [#4184](https://github.com/wasmerio/wasmer/pull/4184) Allow `VirtualTaskManager` to explicitly transfer a module to a blocking task on the threadpool
  - [#4176](https://github.com/wasmerio/wasmer/pull/4176) Js integrity checks
  - [#4171](https://github.com/wasmerio/wasmer/pull/4171) Revive "0-copy module deserialization"
  - [#4173](https://github.com/wasmerio/wasmer/pull/4173) deps: Bump Edge CLI version

## Fixed

  - [#4198](https://github.com/wasmerio/wasmer/pull/4198) chore: fix unavailable document url
  - [#4191](https://github.com/wasmerio/wasmer/pull/4191) Fix invalid access to wasi instance handles in wasix proc_spawn
  - [#4165](https://github.com/wasmerio/wasmer/pull/4165) Fix writing to middle of files and improve performance



## 4.1.2 - 21/08/2023
Another maintenance release, bringing some networking improvements, and centralising all wasmer caches under the same folder.


## Added

  - [#4127](https://github.com/wasmerio/wasmer/pull/4127) Add regression test for path_rename

## Changed

  - [#4164](https://github.com/wasmerio/wasmer/pull/4164) deps: Bump Edge client CLI to 0.1.22
  - [#4163](https://github.com/wasmerio/wasmer/pull/4163) Bumped virtual-net crate to 0.5.0
  - [#4156](https://github.com/wasmerio/wasmer/pull/4156) Disable auto-format of toml files in VS Code
  - [#4141](https://github.com/wasmerio/wasmer/pull/4141) Web http client
  - [#4152](https://github.com/wasmerio/wasmer/pull/4152) Test virtual-net bincode and mpsc only on linux
  - [#4115](https://github.com/wasmerio/wasmer/pull/4115) Browser cfg, smaller WASM, VPN and HTTPS redirect
  - [#4138](https://github.com/wasmerio/wasmer/pull/4138) Upgrade wasmer-wasix to the newest version of wasm-bindgen
  - [#4135](https://github.com/wasmerio/wasmer/pull/4135) Update to time 0.3
  - [#4109](https://github.com/wasmerio/wasmer/pull/4109) Update MSRV to 1.69
  - [#4130](https://github.com/wasmerio/wasmer/pull/4130) Update to tracing-subscriber 0.3

## Fixed

  - [#4160](https://github.com/wasmerio/wasmer/pull/4160) fix: Make the "wasmer cache" command use the same cache directory as the rest of the CLI
  - [#4148](https://github.com/wasmerio/wasmer/pull/4148) fix(cli): Prevent panics in "wasmer login" after API failures



## 4.1.1 - 03/08/2023
Bug-fix release, fixing rename in wasi(x), using newer Rust and some macOS ARM64 speicifc issues, among other things.

## Added

  - [#4120](https://github.com/wasmerio/wasmer/pull/4120) Added proper definition of ucontext for macos/aarch64 to avoid unaligned issue
  - [#4107](https://github.com/wasmerio/wasmer/pull/4107) Added a forced shutdown on tokio runtimes as the STDIN blocks the shu…
  - [#4108](https://github.com/wasmerio/wasmer/pull/4108) Add a temporary workaround for nanasess/setup-chromedriver#190

## Changed

  - [#4123](https://github.com/wasmerio/wasmer/pull/4123) Upgrade Edge CLI dependency
  - [#4109](https://github.com/wasmerio/wasmer/pull/4109) Update MSRV to 1.69
  - [#4111](https://github.com/wasmerio/wasmer/pull/4111) Update login.rs
  - [#4102](https://github.com/wasmerio/wasmer/pull/4102) Update to criterion 0.5

## Fixed

  - [#4121](https://github.com/wasmerio/wasmer/pull/4121) Fix path_rename syscall failing
  - [#4117](https://github.com/wasmerio/wasmer/pull/4117) Fixed an issue where inheritance was inverted
  - [#4096](https://github.com/wasmerio/wasmer/pull/4096) Fix benchmark



## 4.1.0 - 24/07/2023
This version added some more improvements and fixes, with a faster async execution, a new login flow and muliple bugfix to the `--mapdir` command among other things.
More detail in the blog post about the 4.1 Release: https://wasmer.io/posts/wasmer-4.1

## Added

  - [#4081](https://github.com/wasmerio/wasmer/pull/4081) Add WebcHash::parse_hex helper
  - [#4046](https://github.com/wasmerio/wasmer/pull/4046) Add C API function to create Module from Engine instead of Store

## Changed

  - [#4095](https://github.com/wasmerio/wasmer/pull/4095) Bumped the webc version
  - [#4038](https://github.com/wasmerio/wasmer/pull/4038) Clean up the integration tests
  - [#4085](https://github.com/wasmerio/wasmer/pull/4085) Switch to lazily loading a Wasmer package directly from disk
  - [#4084](https://github.com/wasmerio/wasmer/pull/4084) Remove unused atty dependency
  - [#4057](https://github.com/wasmerio/wasmer/pull/4057) login with browser using nonce
  - [#4073](https://github.com/wasmerio/wasmer/pull/4073) deps(wasmer-wasix): Remove two unused dependencies
  - [#4068](https://github.com/wasmerio/wasmer/pull/4068) Enable tokio's "net" feature in the virtual-net crate
  - [#4065](https://github.com/wasmerio/wasmer/pull/4065) deps: Upgrade edge CLI version
  - [#4064](https://github.com/wasmerio/wasmer/pull/4064) Enable the `cfg_doc` feature on docs.rs
  - [#4052](https://github.com/wasmerio/wasmer/pull/4052) Selenium-style tests for wasmer.sh
  - [#4061](https://github.com/wasmerio/wasmer/pull/4061) Use is-terminal to check tty
  - [#4056](https://github.com/wasmerio/wasmer/pull/4056) Update hermit-abi crate, used version was yanked
  - [#4055](https://github.com/wasmerio/wasmer/pull/4055) Use a no-op package loader by default in PluggableRuntime
  - [#4011](https://github.com/wasmerio/wasmer/pull/4011) Move Artifact Register FrameInfo
  - [#4042](https://github.com/wasmerio/wasmer/pull/4042) Disable unused cbindgen feature
  - [#4044](https://github.com/wasmerio/wasmer/pull/4044) Update tempfile crate
  - [#4041](https://github.com/wasmerio/wasmer/pull/4041) Canonicalize host folder on mapdir
  - [#4040](https://github.com/wasmerio/wasmer/pull/4040) Removed error message when a deserializzation error occurs in Artifact
  - [#4032](https://github.com/wasmerio/wasmer/pull/4032) Make the CLI respect the `--token` flag
  - [#4030](https://github.com/wasmerio/wasmer/pull/4030) Remove rustup build dependency
  - [#4031](https://github.com/wasmerio/wasmer/pull/4031) Speed up the module cache 6x by removing LZW compression

## Fixed

  - [#4096](https://github.com/wasmerio/wasmer/pull/4096) Fix benchmark
  - [#4050](https://github.com/wasmerio/wasmer/pull/4050) `epoll` with fs fixes
  - [#4074](https://github.com/wasmerio/wasmer/pull/4074) fix(wasix): Expose FileSystemMapping struct
  - [#4063](https://github.com/wasmerio/wasmer/pull/4063) Fix docstring for MemoryView struct
  - [#4047](https://github.com/wasmerio/wasmer/pull/4047) Fixed create exe when zig is not present
  - [#3970](https://github.com/wasmerio/wasmer/pull/3970) Fixes for wasmer.sh
  - [#4035](https://github.com/wasmerio/wasmer/pull/4035) Fix help text for `use_system_linker` option in `create_exe` CLI command



## 4.0.0 - 22/06/2023

Some more behind-the-scene unification and bug fixe since the last beta, mostly on the CLI, but make sure to check all the other changes of the previous beta and alpha release if you came from last stable version.

## Added

  - [#4013](https://github.com/wasmerio/wasmer/pull/4013) Added a `WasmerEnv` abstraction to the CLI

## Changed

  - [#4021](https://github.com/wasmerio/wasmer/pull/4021) Put all cached artifacts under the `~/.wasmer/cache/` directory
  - [#4018](https://github.com/wasmerio/wasmer/pull/4018) Canonicalize the host path passed to `--mapdir`
  - [#4019](https://github.com/wasmerio/wasmer/pull/4019) Make sure `ExecutableTarget::from_file()` for `*.wasm` files checks the module cache
  - [#4022](https://github.com/wasmerio/wasmer/pull/4022) [CI] Defined wich nightly to use to avoid not ready error
  - [#4001](https://github.com/wasmerio/wasmer/pull/4001) deps: Bump edge CLI
  - [#4012](https://github.com/wasmerio/wasmer/pull/4012) update links in Wasmer's README
  - [#3985](https://github.com/wasmerio/wasmer/pull/3985) Display human-friendly progress when starting `wasmer run`
  - [#4004](https://github.com/wasmerio/wasmer/pull/4004) Ignoring archived package versions when querying the registry
  - [#3980](https://github.com/wasmerio/wasmer/pull/3980) Re-work logging and command-line argument parsing
  - [#3983](https://github.com/wasmerio/wasmer/pull/3983) Introduce WAPM query caching to reduce `wasmer run`'s startup delay
  - [#4002](https://github.com/wasmerio/wasmer/pull/4002) [RISCV64][CI] Use simpler way to install riscv64 packages

## Fixed




## 4.0.0-beta.3 - 15/06/2023

More fixes and improvement to the virtual filesystem, and behind the scene work to unify wasmer tools for this new beta.
RISC-V will now be built on newest Bookworm Debian release.

## Added

  - [#3990](https://github.com/wasmerio/wasmer/pull/3990) fix(cli): Fix deploy command and add some aliases

## Changed

  - [#3998](https://github.com/wasmerio/wasmer/pull/3998) deps: Upgrade edge cli
  - [#3989](https://github.com/wasmerio/wasmer/pull/3989) OverlayFS now has COW
  - [#3991](https://github.com/wasmerio/wasmer/pull/3991) Renamed most appearances of Wapm to Wasmer
  - [#3992](https://github.com/wasmerio/wasmer/pull/3992) [CI][RISCV64] Try to use bookworm debian instead of sid

## Fixed




## 4.0.0-beta.2 - 09/06/2023

This version added a missing `sock_accept` function to WASI preview1, and also greatly improved the filesystem mappings for the resolver, improving the ease of use of the abilty to run packages with the `wasmer run` command.

## Added

  - [#3913](https://github.com/wasmerio/wasmer/pull/3913) Add filesystem mappings to the resolver
  - [#3971](https://github.com/wasmerio/wasmer/pull/3971) Added the missing sock_accept for preview1
  - [#3957](https://github.com/wasmerio/wasmer/pull/3957) Added a test for small Stack (for #3808)
  - [#3956](https://github.com/wasmerio/wasmer/pull/3956) feat(virtual-fs): Add FsError::StorageFull variant

## Changed

  - [#3710](https://github.com/wasmerio/wasmer/pull/3710) Implemented an asyncify based implementation of asynchronous threading
  - [#3950](https://github.com/wasmerio/wasmer/pull/3950) tests: Remove debug_output option from snapshot tests.
  - [#3961](https://github.com/wasmerio/wasmer/pull/3961) chore: Rename wasi dir to wasiX
  - [#3967](https://github.com/wasmerio/wasmer/pull/3967) Use logs instead of timing to determine if the package cache was hit
  - [#3955](https://github.com/wasmerio/wasmer/pull/3955) chore(cli): Remove redundant CLI feature flags
  - [#3953](https://github.com/wasmerio/wasmer/pull/3953) feat: Allow custom manifest file paths in "wasmer publish"
  - [#3433](https://github.com/wasmerio/wasmer/pull/3433) Module.deserialize - accept AsEngineRef
  - [#3946](https://github.com/wasmerio/wasmer/pull/3946) Allow deserialized WAPM JSON even if some version have some NULL infos
  - [#3944](https://github.com/wasmerio/wasmer/pull/3944) Port create-exe and the wasmer C API over to the webc compatibility layer

## Fixed

  - [#3975](https://github.com/wasmerio/wasmer/pull/3975) CLI: Fix deploy Command
  - [#3969](https://github.com/wasmerio/wasmer/pull/3969) fix: Fix incorrect archive construction in wasmer publish



## 4.0.0-beta.1 - 01/06/2023

This version introduce WASIX! The superset of WASI. Go to https://wasix.org for more details.
This version also merged `wasmer run` and `wasmer run-unstable`. Both command still exists but are now exactly the same. `run-unstable` will be removed later, so switch to `wasmer run` if you were using it.

## Added

  - [#3818](https://github.com/wasmerio/wasmer/pull/3818) Added a quickfix for mounting relative directories

## Changed

  - [#3924](https://github.com/wasmerio/wasmer/pull/3924) Rename "wasmer run-unstable" to "wasmer run"
  - [#3938](https://github.com/wasmerio/wasmer/pull/3938) Use unchecked deserialization for `_unchecked` module functions
  - [#3918](https://github.com/wasmerio/wasmer/pull/3918) Workspace metadata
  - [#3919](https://github.com/wasmerio/wasmer/pull/3919) Handle non-200 status codes when downloading packages
  - [#3917](https://github.com/wasmerio/wasmer/pull/3917) Rename WasiRuntime to Runtime

## Fixed

  - [#3933](https://github.com/wasmerio/wasmer/pull/3933) Fix deserialize calls in doc comments
  - [#3936](https://github.com/wasmerio/wasmer/pull/3936) Fix for excessive allocation
  - [#3920](https://github.com/wasmerio/wasmer/pull/3920) Fix filesystem access when running python with `wasmer run-unstable .`
  - [#3867](https://github.com/wasmerio/wasmer/pull/3867) lib/wasi: try quick fix for WasiFS::get_inode_at_path



## 4.0.0-alpha.1 - 25/05/2023

A new major release, with a few breaking changes and the removal of many deprecated functions. Also, some methods previously available from on `Engine` has been moved to the `NativeEngineExt` trait.
Deserialize has changed, with the default version using artifact layout validation. The old function has been renamed to `_unchecked` variant. The speed impact is usually negligeable , but the old function are still present if needed
Many bugfixes and improvements on `wasmer run` and `wasmer run-unstable` (that will be merged into just `wasmer run` soon). `--allow-multiple-wasi-versions` CLI flag is now removed that is now removed and active by default. Logic for `wasmer config set registry.url` that has been fixed to work with `localhost:1234`

## Added

  - [#3879](https://github.com/wasmerio/wasmer/pull/3879) Add compiler features to wai-bindgen-wasmer
  - [#1212](https://github.com/wasmerio/wasmer/pull/1212) Add support for GDB JIT debugging

## Changed

  - [#3852](https://github.com/wasmerio/wasmer/pull/3852) Re-work package resolution and make runners use it
  - [#3910](https://github.com/wasmerio/wasmer/pull/3910) Removed all deprecated functions
  - [#3900](https://github.com/wasmerio/wasmer/pull/3900) lib/virtual-net: tokio::net::lookup_host requires host:port format
  - [#3906](https://github.com/wasmerio/wasmer/pull/3906) WASIX updates after superset changes
  - [#3907](https://github.com/wasmerio/wasmer/pull/3907) Choose subversion of tracing crate in wasix
  - [#3873](https://github.com/wasmerio/wasmer/pull/3873) Initial implementation for poll on pipe
  - [#3904](https://github.com/wasmerio/wasmer/pull/3904) Changed tunables to have a VMConfig settings
  - [#3902](https://github.com/wasmerio/wasmer/pull/3902) TmpFs Memory Usage Tracking and Limiting
  - [#3894](https://github.com/wasmerio/wasmer/pull/3894) Rework Module deserialization functions
  - [#3899](https://github.com/wasmerio/wasmer/pull/3899) enable integration tests with debug binary
  - [#3889](https://github.com/wasmerio/wasmer/pull/3889) Bump llvm
  - [#3881](https://github.com/wasmerio/wasmer/pull/3881) Use `js-default` feature in wai-bindgen-wasmer
  - [#3874](https://github.com/wasmerio/wasmer/pull/3874) refactor!: Memory API Modifications (try_clone/copy, duplicate_in_store)
  - [#3763](https://github.com/wasmerio/wasmer/pull/3763) Implement NativeWasmTypeInto for u32 and u64
  - [#3866](https://github.com/wasmerio/wasmer/pull/3866) Align vfs and vnet directories with crate names
  - [#3864](https://github.com/wasmerio/wasmer/pull/3864) feat(cli): Integrate deploy commands
  - [#3771](https://github.com/wasmerio/wasmer/pull/3771) Asynchronous threading phase2
  - [#3710](https://github.com/wasmerio/wasmer/pull/3710) Implemented an asyncify based implementation of asynchronous threading
  - [#3859](https://github.com/wasmerio/wasmer/pull/3859) Support static linking on Windows
  - [#3856](https://github.com/wasmerio/wasmer/pull/3856) Switch FileSystemCache to use checked artifact deserialization
  - [#3854](https://github.com/wasmerio/wasmer/pull/3854) deps: Upgrade clap to v4
  - [#3849](https://github.com/wasmerio/wasmer/pull/3849) Support MinGW
  - [#3841](https://github.com/wasmerio/wasmer/pull/3841) Introduce a module cache abstraction
  - [#3400](https://github.com/wasmerio/wasmer/pull/3400) Use the wasm_bindgen_downcast crate for downcasting JsValues
  - [#3831](https://github.com/wasmerio/wasmer/pull/3831) Introduce a package resolver
  - [#3843](https://github.com/wasmerio/wasmer/pull/3843) Adapted the publishing script

## Fixed

  - [#3867](https://github.com/wasmerio/wasmer/pull/3867) lib/wasi: try quick fix for WasiFS::get_inode_at_path
  - [#3891](https://github.com/wasmerio/wasmer/pull/3891) Fix typos in docs
  - [#3898](https://github.com/wasmerio/wasmer/pull/3898) fix: Use fallback home directory detection in config commands.
  - [#3861](https://github.com/wasmerio/wasmer/pull/3861) fuzz: fix build error
  - [#3853](https://github.com/wasmerio/wasmer/pull/3853) Fix cargo update



## 3.3.0 - 03/05/2023

Along a few important bugfixes, this version introduce JavaScriptCore support, with full WASI support.

## Added

  - [#3825](https://github.com/wasmerio/wasmer/pull/3825) Added support for JavascriptCore
  - [#3837](https://github.com/wasmerio/wasmer/pull/3837) Cleaned up the GraphQL endpoint logic and added tests
  - [#3833](https://github.com/wasmerio/wasmer/pull/3833) Fix Missing WaiterError export + Add notify/wait to fd_mmap memory
  - [#3819](https://github.com/wasmerio/wasmer/pull/3819) ci: add docs.ts test builds
  - [#3782](https://github.com/wasmerio/wasmer/pull/3782) Add FreeBSD x86 support

## Changed

  - [#3838](https://github.com/wasmerio/wasmer/pull/3838) Re-export `wasmer::EngineRef`
  - [#3820](https://github.com/wasmerio/wasmer/pull/3820) fd_write: Flush on every write to a file
  - [#3813](https://github.com/wasmerio/wasmer/pull/3813) Better errors

## Fixed

  - [#3796](https://github.com/wasmerio/wasmer/pull/3796) Fix returning negative i64 values on web target
  - [#3836](https://github.com/wasmerio/wasmer/pull/3836) Fixed Chinese README example
  - [#3827](https://github.com/wasmerio/wasmer/pull/3827) Made test-build-docs-rs tests docs generation for all crates under libs, and fixed broken ones
  - [#3796](https://github.com/wasmerio/wasmer/pull/3796) Fix returning negative i64 values on web target



## 3.2.1 - 21/04/2023

Some quick fixes for this maintenance release, mainly oriented towards Docs.

## Added

  - [#3782](https://github.com/wasmerio/wasmer/pull/3782) Add FreeBSD x86 support

## Changed

  - [#3786](https://github.com/wasmerio/wasmer/pull/3786) Made Stack Size parametrable (for #3760)
  - [#3805](https://github.com/wasmerio/wasmer/pull/3805) Wasi runner args
  - [#3795](https://github.com/wasmerio/wasmer/pull/3795) Make sure the compile function passed to runners is Send+Sync
  - [#3777](https://github.com/wasmerio/wasmer/pull/3777) Use webc's compat layer in more places

## Fixed

  - [#3806](https://github.com/wasmerio/wasmer/pull/3806) Fixed FunctionEnv migration doc to 3.0
  - [#3804](https://github.com/wasmerio/wasmer/pull/3804) Fixed doc comment in memory_view.rs
  - [#3791](https://github.com/wasmerio/wasmer/pull/3791) Fix doc api



## 3.2.0 - 18/04/2023

A lot of new features since last stable version:
RISCV Support, new Runners (WCGI), API convergence for JS/SYS, and WASI eXtended.

 * RISCV support, on both Cranelift and LLVM Compiler.
 * New Runners, with WCGI as a new one
 * Most WAPM command are now available on the wasmer CLI directly
 * Using Wasmer on `sys` or `js` backend is more transparent now, with now support for running wasmer on`JavaScriptCore`

 * The WASI implementation has undergone a major refactoring, and will continue evolve significantly over the coming months.
    - The old `wasmer_wasi` crate was deprecated.

    - To continue using WASI, please switch to the new `wasmer_wasix` crate, which follows a different versioning scheme than the main Wasmer releases.
    Major changes:
    - An async runtime is now required. The runtime is pluggable, but only tokio is officially supported at the moment.
    - The virtual file system layer was renamed from `wasmer-vfs` to `virtual-fs`, and now is built around an async interface that builds on top of `tokio::{AsyncRead/Write}`

    This refactor will unlock many exciting new features that will be announced soon!
    Just be aware that you will have to expect some instability and frequent releases with potential breaking changes until our new implementation settles. down.

## Added

  - [#3706](https://github.com/wasmerio/wasmer/pull/3706) [CI] Add RISCV in test and build
  - [#3765](https://github.com/wasmerio/wasmer/pull/3765) Added basic support to inode in filestat_get (for #3583 and #3239)
  - [#3751](https://github.com/wasmerio/wasmer/pull/3751) Added some unit testing to singlepass compiler
  - [#3752](https://github.com/wasmerio/wasmer/pull/3752) Added new snapshots tests that use a fixed MIO and TOKIO
  - [#3759](https://github.com/wasmerio/wasmer/pull/3759) Added the wasmer.sh website to the main repo and a CI/CD build test
  - [#3747](https://github.com/wasmerio/wasmer/pull/3747) Added missing crate version bump

## Changed

  - [#3778](https://github.com/wasmerio/wasmer/pull/3778) Upgrade to webc v5.0.0
  - [#3775](https://github.com/wasmerio/wasmer/pull/3775) Ran Cargo update before release
  - [#3774](https://github.com/wasmerio/wasmer/pull/3774) Wasi threads
  - [#3772](https://github.com/wasmerio/wasmer/pull/3772) [DOC] Removed paragraph about default-compiler, as it's doesn't exist anymore
  - [#3766](https://github.com/wasmerio/wasmer/pull/3766) deps: Upgrade memoffset
  - [#3690](https://github.com/wasmerio/wasmer/pull/3690) Introduce a WebcVolumeFileSystem that works for any WEBC version
  - [#3424](https://github.com/wasmerio/wasmer/pull/3424) Use "wasmer --version --verbose" in the issue template
  - [#3757](https://github.com/wasmerio/wasmer/pull/3757) WASI without VM internals
  - [#3758](https://github.com/wasmerio/wasmer/pull/3758) More and better snapshot tests
  - [#3756](https://github.com/wasmerio/wasmer/pull/3756) Aarch64 stackprobe
  - [#3753](https://github.com/wasmerio/wasmer/pull/3753) Upgrade webc from webc 5.0.0-rc.5 to webc 5.0.0-rc.6

## Fixed

  - [#3767](https://github.com/wasmerio/wasmer/pull/3767) Fix Snapshot0 fd_filestat_get and path_filestat_get
  - [#3723](https://github.com/wasmerio/wasmer/pull/3723) Fix Wait/Notify opcode, the waiters hashmap is now on the Memory itself
  - [#3749](https://github.com/wasmerio/wasmer/pull/3749) Fixed publish script with latest needed changes
  - [#3750](https://github.com/wasmerio/wasmer/pull/3750) Fixes Chinese docs links (for #3746)
  - [#3761](https://github.com/wasmerio/wasmer/pull/3761) Fix error in Korean translation



## 3.2.0-beta.2 - 05/04/2023

Bug fixes for this beta release, and some exciting new possibilities with the `run-unstable` function from the CLI tools.
 - New wcgi runner
 - Caching of runners
 - Many fixes to wasix

Known issue: RISC-V build is currently broken and will be fixes on next version

## Added

  - [#3731](https://github.com/wasmerio/wasmer/pull/3731) Added missing ASM instructions for wasix on singlepass

## Changed

  - [#3743](https://github.com/wasmerio/wasmer/pull/3743) Bumped crates version pre-beta.2 release
  - [#3742](https://github.com/wasmerio/wasmer/pull/3742) Update versions of wcgi
  - [#3739](https://github.com/wasmerio/wasmer/pull/3739) Use cache in runners
  - [#3735](https://github.com/wasmerio/wasmer/pull/3735) [CI] More WAPM_DEV_TOKEN check for CI
  - [#3734](https://github.com/wasmerio/wasmer/pull/3734) Updated spin to 0.9.8
  - [#3733](https://github.com/wasmerio/wasmer/pull/3733) [CI] Use early exit for wasmer_init_works_1 and wasmer_init_works_2 when WAPM_DEV_TOKEN is empty
  - [#3693](https://github.com/wasmerio/wasmer/pull/3693) Validate Module Artifacts (CLI + WASIX ModuleCache)
  - [#3724](https://github.com/wasmerio/wasmer/pull/3724) ci: Cancel previous workflow runs when new commits are pushed
  - [#3722](https://github.com/wasmerio/wasmer/pull/3722) Revert #3715
  - [#3718](https://github.com/wasmerio/wasmer/pull/3718) Apply CGI environment variables in the correct order
  - [#3716](https://github.com/wasmerio/wasmer/pull/3716) Move Package Build/Publish Logic to wasmer-registry

## Fixed

  - [#3740](https://github.com/wasmerio/wasmer/pull/3740) Fix fuzz build
  - [#3741](https://github.com/wasmerio/wasmer/pull/3741) Fixed an issue where blocking polls were being treated as nonblocking
  - [#3737](https://github.com/wasmerio/wasmer/pull/3737) Fixed FileSystem createdir when parent_inode is an ArcDirectory
  - [#3736](https://github.com/wasmerio/wasmer/pull/3736) Fixed Vectored IO when a partial operation occurs
  - [#3726](https://github.com/wasmerio/wasmer/pull/3726) fix CI-Badge on README.md
  - [#3687](https://github.com/wasmerio/wasmer/pull/3687) Master with fixes
  - [#3719](https://github.com/wasmerio/wasmer/pull/3719) Fixes a bug which causes libc to thrash the futex_wake_all
  - [#3715](https://github.com/wasmerio/wasmer/pull/3715) Fix annotation deserializing for the Wasi and Wcgi runners
  - [#3704](https://github.com/wasmerio/wasmer/pull/3704) Fix the absolute/relative path weirdness when setting up WASI-based runner filesystems
  - [#3705](https://github.com/wasmerio/wasmer/pull/3705) Fixes post 3.2.0-beta.1 release (but needed for the crates publication)



## 3.2.0-beta.1 - 22/03/2023

Lots of new things in the release!

  - WASIX, the new WASI eXtended library, that alows WASM to use Threads, Network, and more in both Runtime and inside a Browser.
  - RISC-V support, on both LLVM and Cranelift compiler.
  - Improved CLI, with more commands and more WAPM integration.
  - Lots of behind the scene refactoring, with sys/js convergence on the API side, and a lot of crate changes.

## Added

  - [#3700](https://github.com/wasmerio/wasmer/pull/3700) Added back all removed function for Engine to avoid API breaking changes
  - [#3654](https://github.com/wasmerio/wasmer/pull/3654) [SINGLEPASS] Add more ROR emitter to ARM64 backend (for #3647)
  - [#3664](https://github.com/wasmerio/wasmer/pull/3664) feat(wasi): Add CapabilityThreading with max thread count
  - [#3556](https://github.com/wasmerio/wasmer/pull/3556) Use standard API for js and sys for Module. Added Engine in js
  - [#3635](https://github.com/wasmerio/wasmer/pull/3635) Added some fixes for the browser version to work again
  - [#3626](https://github.com/wasmerio/wasmer/pull/3626) add basic coredump generation
  - [#3631](https://github.com/wasmerio/wasmer/pull/3631) [CI] Add opened type for pull_request on test.yaml
  - [#3612](https://github.com/wasmerio/wasmer/pull/3612) Added conveniance function FunctionEnvMut::data_and_store_mut
  - [#3536](https://github.com/wasmerio/wasmer/pull/3536) Add documentation for create-exe syntax
  - [#3512](https://github.com/wasmerio/wasmer/pull/3512) Add unit test for verifying that caching works when running packages twice
  - [#3490](https://github.com/wasmerio/wasmer/pull/3490) Fix JS sample code by adding "&mut store"

## Changed

  - [#3244](https://github.com/wasmerio/wasmer/pull/3244) Feat riscv llvm and cranelift
  - [#3650](https://github.com/wasmerio/wasmer/pull/3650) Wasmer run-unstable
  - [#3692](https://github.com/wasmerio/wasmer/pull/3692) PluggableRuntime Cleanup
  - [#3681](https://github.com/wasmerio/wasmer/pull/3681) (feat) Ability to supply your own shared tokio runtime
  - [#3689](https://github.com/wasmerio/wasmer/pull/3689) Increased CURRENT_VERSION to 3
  - [#3691](https://github.com/wasmerio/wasmer/pull/3691) Don't error out on webc/wasi run when exiting normaly
  - [#3683](https://github.com/wasmerio/wasmer/pull/3683) Renamed some crates and changed their version to 0.1.0
  - [#3677](https://github.com/wasmerio/wasmer/pull/3677) Overlay FileSystem
  - [#3580](https://github.com/wasmerio/wasmer/pull/3580) Make memoryView lifetime linked to StoreRef instead of Memory
  - [#3682](https://github.com/wasmerio/wasmer/pull/3682) Update wasmparser to v0.95
  - [#3676](https://github.com/wasmerio/wasmer/pull/3676) Switch `wasmer_vfs::host_fs` tests over to using temp directories
  - [#3670](https://github.com/wasmerio/wasmer/pull/3670) Bumped cranelift to 0.91.1 following a critical security alert
  - [#3666](https://github.com/wasmerio/wasmer/pull/3666) refactor(wasi): Move capabilities into root submodule
  - [#3644](https://github.com/wasmerio/wasmer/pull/3644) Improvements to the tracing and logging in wasmer
  - [#3656](https://github.com/wasmerio/wasmer/pull/3656) Re-enable (most) WASIX Snapshot Tests
  - [#3658](https://github.com/wasmerio/wasmer/pull/3658) [CI] This should setup LLVM on Windows correctly this time
  - [#3655](https://github.com/wasmerio/wasmer/pull/3655) [CI] Manually setup LLVM because Custom build doesn't include llvm-config.exe
  - [#3599](https://github.com/wasmerio/wasmer/pull/3599) Move the `wcgi-runner` into Wasmer
  - [#3646](https://github.com/wasmerio/wasmer/pull/3646) deps: Remove tempdir dependency
  - [#3641](https://github.com/wasmerio/wasmer/pull/3641) Upgrade LLVM to 14.0
  - [#3640](https://github.com/wasmerio/wasmer/pull/3640) Zero memory copies on IO
  - [#3636](https://github.com/wasmerio/wasmer/pull/3636) Removed choice for object-format in create-exe and create-obj commands
  - [#3637](https://github.com/wasmerio/wasmer/pull/3637) Remove unused import in lib.rs example
  - [#3632](https://github.com/wasmerio/wasmer/pull/3632) [CI] Another attempt to get Test on external PR
  - [#3628](https://github.com/wasmerio/wasmer/pull/3628) Registry: Implement App Publishing and Token Generation
  - [#3627](https://github.com/wasmerio/wasmer/pull/3627) wasi: Remove vbus leftovers and improve task joining
  - [#3620](https://github.com/wasmerio/wasmer/pull/3620) ci: Remove deleted vbus crate from build workflow
  - [#3422](https://github.com/wasmerio/wasmer/pull/3422) WASIX
  - [#3539](https://github.com/wasmerio/wasmer/pull/3539) Document release process and update release script
  - [#3593](https://github.com/wasmerio/wasmer/pull/3593) Update rkyv 0.7.40 and prettytable-rs to 0.10.0
  - [#3585](https://github.com/wasmerio/wasmer/pull/3585) CI: Purge Cache with Scheduled Action
  - [#3590](https://github.com/wasmerio/wasmer/pull/3590) Optimize getting byteLength in MemoryView::new
  - [#3586](https://github.com/wasmerio/wasmer/pull/3586) Make AsStoreRef and friends work for anything that implements Deref
  - [#3582](https://github.com/wasmerio/wasmer/pull/3582) bump inkwell to 0.1.1
  - [#3577](https://github.com/wasmerio/wasmer/pull/3577) Use Cloudflare as CI Cache
  - [#3353](https://github.com/wasmerio/wasmer/pull/3353) Speed up CI
  - [#3564](https://github.com/wasmerio/wasmer/pull/3564) Updated Cranelift to 0.91
  - [#3537](https://github.com/wasmerio/wasmer/pull/3537) bump inkwell to 0.1.0
  - [#3562](https://github.com/wasmerio/wasmer/pull/3562) Improved handling of wasmer-headless generation and use on local run
  - [#3563](https://github.com/wasmerio/wasmer/pull/3563) Revert "bump inkwell to 0.1.0"
  - [#3558](https://github.com/wasmerio/wasmer/pull/3558) Reduce libwasmer-headless size
  - [#3552](https://github.com/wasmerio/wasmer/pull/3552) create-exe: link with libwasmer-headless
  - [#3547](https://github.com/wasmerio/wasmer/pull/3547) Remove setting memory for WASI compatibility
  - [#3531](https://github.com/wasmerio/wasmer/pull/3531) Reenable create-exe tests
  - [#3534](https://github.com/wasmerio/wasmer/pull/3534) Use JS VM store instead of artificially replicate it
  - [#3532](https://github.com/wasmerio/wasmer/pull/3532) VMInstance cannot be Clone

## Fixed

  - [#3663](https://github.com/wasmerio/wasmer/pull/3663) Dash fixes and pthreads
  - [#3684](https://github.com/wasmerio/wasmer/pull/3684) Fix: module cache delta overflow
  - [#3680](https://github.com/wasmerio/wasmer/pull/3680) Fix for the vectored IO
  - [#3668](https://github.com/wasmerio/wasmer/pull/3668) Fix doc build
  - [#3665](https://github.com/wasmerio/wasmer/pull/3665) Fix doc, threads are enabled by default now
  - [#3662](https://github.com/wasmerio/wasmer/pull/3662) Why is it so difficult to type LLVM_SYS_140_PREFIX
  - [#3661](https://github.com/wasmerio/wasmer/pull/3661) [CI] New attempt at fixing the Windows build on the CI
  - [#3659](https://github.com/wasmerio/wasmer/pull/3659) Fixed building with with just the sys feature
  - [#3648](https://github.com/wasmerio/wasmer/pull/3648) Fix CI and llvm detection
  - [#3643](https://github.com/wasmerio/wasmer/pull/3643) fix(wasi): Memory leak due to cyclical WasiControlPlane references
  - [#3639](https://github.com/wasmerio/wasmer/pull/3639) wasi: Thread Lifecycle Fix
  - [#3630](https://github.com/wasmerio/wasmer/pull/3630) Fix linter
  - [#3591](https://github.com/wasmerio/wasmer/pull/3591) fix: experimental io
  - [#3629](https://github.com/wasmerio/wasmer/pull/3629) Webc Inheritance Fixes
  - [#3598](https://github.com/wasmerio/wasmer/pull/3598) Wasix multithreading fix
  - [#3601](https://github.com/wasmerio/wasmer/pull/3601) Update CI script to fix deprecation notices
  - [#3623](https://github.com/wasmerio/wasmer/pull/3623) ci: Fix build.yaml workflow
  - [#3581](https://github.com/wasmerio/wasmer/pull/3581) Fix WASI example that initialized wasi_config 2 times
  - [#3584](https://github.com/wasmerio/wasmer/pull/3584) Wasix major fixes and tweaks
  - [#3557](https://github.com/wasmerio/wasmer/pull/3557) Fix make-test-integration CLI producing files when running locally
  - [#3566](https://github.com/wasmerio/wasmer/pull/3566) Fix create-exe with create-obj when using dashes in atom names
  - [#3573](https://github.com/wasmerio/wasmer/pull/3573) Fix/compile not in memory
  - [#3569](https://github.com/wasmerio/wasmer/pull/3569) Fix error message due to wapm.dev backend change
  - [#3551](https://github.com/wasmerio/wasmer/pull/3551) Fix markup in Japanese README
  - [#3493](https://github.com/wasmerio/wasmer/pull/3493) Fixed some memory leak issue with InstanceHandle
  - [#3523](https://github.com/wasmerio/wasmer/pull/3523) Fix Windows-GNU distribution



## 3.2.0-alpha.1 - 23/01/2023

## Added

  - [#3477](https://github.com/wasmerio/wasmer/pull/3477) Added support for Wasm Module custom sections in js
  - [#3462](https://github.com/wasmerio/wasmer/pull/3462) [SINGLEPASS] Added a special case on SSE4.2 backend when dst == src1

## Changed

  - [#3511](https://github.com/wasmerio/wasmer/pull/3511) Incremented CURRENT_VERSION, so all cache will be invalidate and be rebuilt with the 3.2 version
  - [#3498](https://github.com/wasmerio/wasmer/pull/3498) Wasix Control Plane - Thread Limit + Cleanup
  - [#3465](https://github.com/wasmerio/wasmer/pull/3465) Remove assert in sse_round_fn and handle case where src2 is in memory
  - [#3494](https://github.com/wasmerio/wasmer/pull/3494) Update wasmer-toml version
  - [#3426](https://github.com/wasmerio/wasmer/pull/3426) WASIX Preparation
  - [#3480](https://github.com/wasmerio/wasmer/pull/3480) Ignore Create-exe with serialize test, something is wrong with the generated exe
  - [#3471](https://github.com/wasmerio/wasmer/pull/3471) Rename `WasiState::new()` to `WasiState::builder()`
  - [#3430](https://github.com/wasmerio/wasmer/pull/3430) Implement support for multiple commands in one native executable
  - [#3455](https://github.com/wasmerio/wasmer/pull/3455) Remove hardcoded rust-toolchain and use panic=abort on windows-gnu
  - [#3353](https://github.com/wasmerio/wasmer/pull/3353) Speed up CI
  - [#3433](https://github.com/wasmerio/wasmer/pull/3433) Module.deserialize - accept AsEngineRef
  - [#3428](https://github.com/wasmerio/wasmer/pull/3428) Implement wasmer config
  - [#3432](https://github.com/wasmerio/wasmer/pull/3432) Amend changes to wasmer init
  - [#3439](https://github.com/wasmerio/wasmer/pull/3439) Use GNU/Linux frame registration code for FreeBSD too
  - [#3324](https://github.com/wasmerio/wasmer/pull/3324) Implement wasmer init and wasmer publish
  - [#3431](https://github.com/wasmerio/wasmer/pull/3431) Revert "Implement wasmer init and wasmer publish"

## Fixed

  - [#3483](https://github.com/wasmerio/wasmer/pull/3483) Fix feature flags for make-build-wasmer-headless
  - [#3496](https://github.com/wasmerio/wasmer/pull/3496) Fixed create-exe tests for object-format serialized
  - [#3479](https://github.com/wasmerio/wasmer/pull/3479) This should fix CI build of CAPI Headless
  - [#3473](https://github.com/wasmerio/wasmer/pull/3473) Fix wasm publish validation
  - [#3467](https://github.com/wasmerio/wasmer/pull/3467) Fix wasmer-wasi-js compilation
  - [#3443](https://github.com/wasmerio/wasmer/pull/3443) Fix fuzz errors
  - [#3456](https://github.com/wasmerio/wasmer/pull/3456) Fix wasmer-rust readme example
  - [#3440](https://github.com/wasmerio/wasmer/pull/3440) Fix CI for external collaborator PRs
  - [#3427](https://github.com/wasmerio/wasmer/pull/3427) Fix cargo-deny failing on webc crate
  - [#3423](https://github.com/wasmerio/wasmer/pull/3423) Fix minor typo
  - [#3419](https://github.com/wasmerio/wasmer/pull/3419) Fix CHANGELOG generation to list by PR merged date, not created date



## Fixed

  - [#3439](https://github.com/wasmerio/wasmer/pull/3439) Use GNU/Linux frame registration code for FreeBSD too
  - [#3448](https://github.com/wasmerio/wasmer/pull/3448) Fix `make install-capi-lib` install paths

## 3.1.0 - 12/12/2022

## Added

  - [#3403](https://github.com/wasmerio/wasmer/pull/3403) Add wasm_importtype_copy to C API

## Changed

  - [#3416](https://github.com/wasmerio/wasmer/pull/3416) Download and install packages via .tar.gz URLs and improve installation error message
  - [#3402](https://github.com/wasmerio/wasmer/pull/3402) Do not run first command of wapm file and print all commands instead
  - [#3400](https://github.com/wasmerio/wasmer/pull/3400) Use the wasm_bindgen_downcast crate for downcasting JsValues
  - [#3363](https://github.com/wasmerio/wasmer/pull/3363) Store Used CpuFeature in Artifact instead of Present CpuFeatures for Singlepass
  - [#3378](https://github.com/wasmerio/wasmer/pull/3378) Introduced EngineRef and AsEngineRef trait
  - [#3386](https://github.com/wasmerio/wasmer/pull/3386) Restore Support For All Wasi Clock Types
  - [#3153](https://github.com/wasmerio/wasmer/pull/3153) SharedMemory & Atomics

## Fixed

  - [#3415](https://github.com/wasmerio/wasmer/pull/3415) Fix singlepass for Aarch64
  - [#3395](https://github.com/wasmerio/wasmer/pull/3395) Fix create-exe to be able to cross-compile on Windows
  - [#3396](https://github.com/wasmerio/wasmer/pull/3396) Fix build doc and minimum-sys build

## 3.0.2 - 25/11/2022

## Added

  - [#3364](https://github.com/wasmerio/wasmer/pull/3364) Added the actual LZCNT / TZCNT implementation

## Changed

  - [#3365](https://github.com/wasmerio/wasmer/pull/3365) Improve FreeBSD support
  - [#3368](https://github.com/wasmerio/wasmer/pull/3368) Remove wasi conditional compilation from wasmer-registry
  - [#3367](https://github.com/wasmerio/wasmer/pull/3367) Change LLVM detection in Makefile

## Fixed

  - [#3370](https://github.com/wasmerio/wasmer/pull/3370) Fix wasmer run not interpreting URLs correctly + display fixes
  - [#3371](https://github.com/wasmerio/wasmer/pull/3371) Fix cargo binstall


## 3.0.1 - 23/11/2022

## Added

  - [#3361](https://github.com/wasmerio/wasmer/pull/3361) Give users feedback when they are running "wasmer add ..."

## Changed

  - [#3360](https://github.com/wasmerio/wasmer/pull/3360) Introduce a "wasmer_registry::queries" module with all GraphQL queries
  - [#3355](https://github.com/wasmerio/wasmer/pull/3355) Fetch the pirita download URL
  - [#3344](https://github.com/wasmerio/wasmer/pull/3344) Revert #3145
  - [#3302](https://github.com/wasmerio/wasmer/pull/3302) Some Refactor of Singlepass compiler to have better error and cpu features handling
  - [#3296](https://github.com/wasmerio/wasmer/pull/3296) Use the right collection when parsing type section
  - [#3292](https://github.com/wasmerio/wasmer/pull/3292) Precompute offsets in VMOffsets
  - [#3290](https://github.com/wasmerio/wasmer/pull/3290) Limit the use of clone when handling Compilation object
  - [#3316](https://github.com/wasmerio/wasmer/pull/3316) Implement wasmer whoami
  - [#3341](https://github.com/wasmerio/wasmer/pull/3341) Update CHANGELOG.md

## Fixed

  - [#3342](https://github.com/wasmerio/wasmer/pull/3342) Fixes for 3.0.0 release

## 3.0.0 - 20/11/2022

## Added

  - [#3339](https://github.com/wasmerio/wasmer/pull/3339) Fixes for wasmer login / wasmer add
  - [#3337](https://github.com/wasmerio/wasmer/pull/3337) Add automation script to automate deploying releases on GitHub
  - [#3338](https://github.com/wasmerio/wasmer/pull/3338) Re-add codecov to get coverage reports

## Changed

  - [#3295](https://github.com/wasmerio/wasmer/pull/3295) Implement wasmer run {url}

## Fixed


## 3.0.0-rc.3 - 2022/11/18

## Added

  - [#3314](https://github.com/wasmerio/wasmer/pull/3314) Add windows-gnu workflow

## Changed

  - [#3317](https://github.com/wasmerio/wasmer/pull/3317) Port "wapm install" to Wasmer
  - [#3318](https://github.com/wasmerio/wasmer/pull/3318) Bump the MSRV to 1.63
  - [#3319](https://github.com/wasmerio/wasmer/pull/3319) Disable 'Test integration CLI' on CI for the Windows platform as it's not working at all
  - [#3297](https://github.com/wasmerio/wasmer/pull/3297) Implement wasmer login
  - [#3311](https://github.com/wasmerio/wasmer/pull/3311) Export Module::IoCompileError as it's an error returned by an exported function
  - [#2800](https://github.com/wasmerio/wasmer/pull/2800) RISC-V support
  - [#3293](https://github.com/wasmerio/wasmer/pull/3293) Removed call to to_vec() on assembler.finalise()
  - [#3288](https://github.com/wasmerio/wasmer/pull/3288) Rollback all the TARGET_DIR changes
  - [#3284](https://github.com/wasmerio/wasmer/pull/3284) Makefile now handle TARGET_DIR env. var. for build too
  - [#3276](https://github.com/wasmerio/wasmer/pull/3276) Remove unnecessary checks to test internet connection
  - [#3266](https://github.com/wasmerio/wasmer/pull/3266) Return ENotCapable error when accessing unknown files on root (for #3263 and #3264)
  - [#3275](https://github.com/wasmerio/wasmer/pull/3275) Disable printing "local package ... not found" in release mode
  - [#3273](https://github.com/wasmerio/wasmer/pull/3273) Undo Makefile commit

## Fixed

  - [#3299](https://github.com/wasmerio/wasmer/pull/3299) Fix "create-exe" for windows-x86_64 target
  - [#3294](https://github.com/wasmerio/wasmer/pull/3294) Fix test sys yaml syntax
  - [#3287](https://github.com/wasmerio/wasmer/pull/3287) Fix Makefile with TARGET_DIR end with release folder, removing it
  - [#3286](https://github.com/wasmerio/wasmer/pull/3286) Fix Makefile with TARGET_DIR end with release folder
  - [#3285](https://github.com/wasmerio/wasmer/pull/3285) Fix CI to setup TARGET_DIR to target/release directly
  - [#3277](https://github.com/wasmerio/wasmer/pull/3277) Fix red CI on master


## 3.0.0-rc.2 - 2022/11/02

## Added


## Changed

  - [#3258](https://github.com/wasmerio/wasmer/pull/3258) Migrate pirita / native executables feature from wasmer-private

## Fixed

  - [#3268](https://github.com/wasmerio/wasmer/pull/3268) Fix fd_right nightly test to avoid foo.txt file leftover
  - [#3260](https://github.com/wasmerio/wasmer/pull/3260) Fix bug in wasmer run
  - [#3257](https://github.com/wasmerio/wasmer/pull/3257) Fix linux-aarch64 build


## 3.0.0-rc.1 - 2022/10/25

## Added

  - [#3222](https://github.com/wasmerio/wasmer/pull/3222) Add function to retrieve function name from wasm_frame_t
  - [#3240](https://github.com/wasmerio/wasmer/pull/3240) Fix filesystem rights on WASI, add integration test for file permissions
  - [#3238](https://github.com/wasmerio/wasmer/pull/3238) Fixed main README ocaml homepage link and added ocaml in other language README
  - [#3145](https://github.com/wasmerio/wasmer/pull/3145) C-API: add functions to overwrite stdin / stdout / stderr handlers

## Changed

  - [#3215](https://github.com/wasmerio/wasmer/pull/3215) Update wasmer --version logic, integrate wapm-cli
  - [#3248](https://github.com/wasmerio/wasmer/pull/3248) Move loupe CHANGELOG entry from 2.3.0 to 3.x
  - [#3230](https://github.com/wasmerio/wasmer/pull/3230) Remove test if dest file exist on path_rename wasi syscall (for #3228)
  - [#3061](https://github.com/wasmerio/wasmer/pull/3061) Removed trailing zero in WASI::fd_prestat_dir_name name return (for #3025)
  - [#3223](https://github.com/wasmerio/wasmer/pull/3223) Delete lib/wasi-types-generated directory
  - [#3178](https://github.com/wasmerio/wasmer/pull/3178) Feat enhanced tinytunable test
  - [#3177](https://github.com/wasmerio/wasmer/pull/3177) Auto-generate wasi-types from .wit files
  - [#3218](https://github.com/wasmerio/wasmer/pull/3218) Seal `HostFunctionKind`

## Fixed

  - [#3221](https://github.com/wasmerio/wasmer/pull/3221) Fix #3197
  - [#3229](https://github.com/wasmerio/wasmer/pull/3229) Fixed version to nightly-2022-10-09 for the CI build Minimal Wasmer Headless again
  - [#3227](https://github.com/wasmerio/wasmer/pull/3227) Fixed version to nightly-2022-10-09 for the CI build Minimal Wasmer Headless
  - [#3226](https://github.com/wasmerio/wasmer/pull/3226) Fixed version to nightly-2002-10-09 for the CI build Minimal Wasmer Headless
  - [#3211](https://github.com/wasmerio/wasmer/pull/3211) fix popcnt for aarch64
  - [#3204](https://github.com/wasmerio/wasmer/pull/3204) Fixed a typo in README


## 3.0.0-beta.2 - 2022/09/26

## Added

  - [#3176](https://github.com/wasmerio/wasmer/pull/3176) Add support for `cargo-binstall`
  - [#3141](https://github.com/wasmerio/wasmer/pull/3141) The API breaking changes from future WASIX/Network/Threading addition
  - [#3119](https://github.com/wasmerio/wasmer/pull/3119) Added LinearMemory trait
  - [#3117](https://github.com/wasmerio/wasmer/pull/3117) Add tests for wasmer-cli create-{exe,obj} commands
  - [#3101](https://github.com/wasmerio/wasmer/pull/3101) CI/build.yaml: add libwasmer headless in default distribution
  - [#3090](https://github.com/wasmerio/wasmer/pull/3090) Added version to the wasmer cli
  - [#3089](https://github.com/wasmerio/wasmer/pull/3089) Add wasi_* C-API function changes in migration guide for 3.0.0
  - [#3076](https://github.com/wasmerio/wasmer/pull/3076) Add support for cross-compiling in create-exe with zig cc WIP
  - [#3072](https://github.com/wasmerio/wasmer/pull/3072) Add back `Function::*_with_env(…)`
  - [#3048](https://github.com/wasmerio/wasmer/pull/3048) Add cloudcompiler.yaml
  - [#3068](https://github.com/wasmerio/wasmer/pull/3068) create-{exe,obj}: add documentations and header file generation for create-obj
  - [#3065](https://github.com/wasmerio/wasmer/pull/3065) Added '.' and '..' special folder t WASI fd_readdir return (for #3033)

## Changed

  - [#3184](https://github.com/wasmerio/wasmer/pull/3184) Test libwasmer.dll on Windows
  - [#3164](https://github.com/wasmerio/wasmer/pull/3164) Synchronize between -sys and -js tests
  - [#3165](https://github.com/wasmerio/wasmer/pull/3165) Initial port of make test-js-core (port wasmer API to core)
  - [#3138](https://github.com/wasmerio/wasmer/pull/3138) Js imports revamp
  - [#3142](https://github.com/wasmerio/wasmer/pull/3142) Bump rust toolchain
  - [#3116](https://github.com/wasmerio/wasmer/pull/3116) Multithreading, full networking and RPC for WebAssembly
  - [#3130](https://github.com/wasmerio/wasmer/pull/3130) Remove panics from Artifact::deserialize
  - [#3134](https://github.com/wasmerio/wasmer/pull/3134) Bring libwasmer-headless.a from 22MiB to 7.2MiB (on my machine)
  - [#3131](https://github.com/wasmerio/wasmer/pull/3131) Update for migration-to-3.0.0 for MemoryView changes
  - [#3123](https://github.com/wasmerio/wasmer/pull/3123) Lower libwasmer headless size
  - [#3132](https://github.com/wasmerio/wasmer/pull/3132) Revert "Lower libwasmer headless size"
  - [#3128](https://github.com/wasmerio/wasmer/pull/3128) scripts/publish.py: validate crates version before publishing
  - [#3126](https://github.com/wasmerio/wasmer/pull/3126) scripts/publish.py: replace toposort dependency with python std graphlib module
  - [#3122](https://github.com/wasmerio/wasmer/pull/3122) Update Cargo.lock dependencies
  - [#3118](https://github.com/wasmerio/wasmer/pull/3118) Refactor Artifact enum into a struct
  - [#3114](https://github.com/wasmerio/wasmer/pull/3114) Implemented shared memory for Wasmer in preparation for multithreading
  - [#3104](https://github.com/wasmerio/wasmer/pull/3104) Re-enabled ExternRef tests
  - [#3103](https://github.com/wasmerio/wasmer/pull/3103) create-exe: prefer libwasmer headless when cross-compiling
  - [#3097](https://github.com/wasmerio/wasmer/pull/3097) MemoryView lifetime tied to memory and not StoreRef
  - [#3095](https://github.com/wasmerio/wasmer/pull/3095) create-exe: list supported cross-compilation target triples in help …
  - [#3096](https://github.com/wasmerio/wasmer/pull/3096) create-exe: use cached wasmer tarballs for network fetches
  - [#3083](https://github.com/wasmerio/wasmer/pull/3083) Disable wasm build in build CI
  - [#3081](https://github.com/wasmerio/wasmer/pull/3081) 3.0.0-beta release
  - [#3079](https://github.com/wasmerio/wasmer/pull/3079) Migrate to clap from structopt
  - [#3075](https://github.com/wasmerio/wasmer/pull/3075) Remove __wbindgen_thread_id
  - [#3074](https://github.com/wasmerio/wasmer/pull/3074) Update chrono to 0.4.20, avoiding RUSTSEC-2020-0159
  - [#3070](https://github.com/wasmerio/wasmer/pull/3070) wasmer-cli: Allow create-exe to receive a static object as input
  - [#3069](https://github.com/wasmerio/wasmer/pull/3069) Remove native feature entry from docs.rs metadata
  - [#3057](https://github.com/wasmerio/wasmer/pull/3057) wasmer-cli: create-obj command
  - [#3060](https://github.com/wasmerio/wasmer/pull/3060) CI: Unset rustup override after usage instead of setting it to stable

## Fixed

  - [#3192](https://github.com/wasmerio/wasmer/pull/3192) fix the typos
  - [#3185](https://github.com/wasmerio/wasmer/pull/3185) Fix `wasmer compile` command for non-x86 target
  - [#3129](https://github.com/wasmerio/wasmer/pull/3129) Fix differences between -sys and -js API
  - [#3137](https://github.com/wasmerio/wasmer/pull/3137) Fix cache path not being present during installation of cross-tarball
  - [#3115](https://github.com/wasmerio/wasmer/pull/3115) Fix static object signature deserialization
  - [#3093](https://github.com/wasmerio/wasmer/pull/3093) Fixed a potential issue when renaming a file
  - [#3088](https://github.com/wasmerio/wasmer/pull/3088) Fixed an issue when renaming a file from a preopened dir directly (for 3084)
  - [#3078](https://github.com/wasmerio/wasmer/pull/3078) Fix errors from "make lint"
  - [#3052](https://github.com/wasmerio/wasmer/pull/3052) Fixed a memory corruption issue with JS memory operations that were r…
  - [#3058](https://github.com/wasmerio/wasmer/pull/3058) Fix trap tracking


## 3.0.0-alpha.4 - 2022/07/28

## Added

  - [#3035](https://github.com/wasmerio/wasmer/pull/3035) Added a simple divide by zero trap wast test (for #1899)
  - [#3008](https://github.com/wasmerio/wasmer/pull/3008) Add check-public-api.yaml workflow
  - [#3021](https://github.com/wasmerio/wasmer/pull/3021) Added back some needed relocation for arm64 llvm compiler
  - [#2982](https://github.com/wasmerio/wasmer/pull/2982) Add a `rustfmt.toml` file to the repository
  - [#2953](https://github.com/wasmerio/wasmer/pull/2953) Makefile: add `check` target
  - [#2952](https://github.com/wasmerio/wasmer/pull/2952) CI: add make build-wasmer-wasm test

## Changed

  - [#3051](https://github.com/wasmerio/wasmer/pull/3051) Updated Crenelift to v0.86.1
  - [#3038](https://github.com/wasmerio/wasmer/pull/3038) Re-introduce create-exe to wasmer-cli v3.0
  - [#3049](https://github.com/wasmerio/wasmer/pull/3049) Disable traps::trap_display_multi_module test for Windows+singlepass
  - [#3047](https://github.com/wasmerio/wasmer/pull/3047) Improved EngineBuilder API
  - [#3046](https://github.com/wasmerio/wasmer/pull/3046) Merge Backend into EngineBuilder and refactor feature flags
  - [#3039](https://github.com/wasmerio/wasmer/pull/3039) Improved hashing/ids of function envs
  - [#3029](https://github.com/wasmerio/wasmer/pull/3029) Remove Engine, Artifact traits, merge all Engines into one, make everything rkyv serialazable
  - [#2892](https://github.com/wasmerio/wasmer/pull/2892) Implement new Context API for Wasmer 3.0
  - [#3031](https://github.com/wasmerio/wasmer/pull/3031) Update docs/migration_to_3.0.0.md
  - [#3030](https://github.com/wasmerio/wasmer/pull/3030) Remove cranelift dependency from wasmer-wasi
  - [#3028](https://github.com/wasmerio/wasmer/pull/3028) Ctx store rename
  - [#3023](https://github.com/wasmerio/wasmer/pull/3023) Changed CI rust install action to dtolnay one
  - [#3013](https://github.com/wasmerio/wasmer/pull/3013) Context api refactor
  - [#2999](https://github.com/wasmerio/wasmer/pull/2999) Support --invoke option for emscripten files without _start function
  - [#3003](https://github.com/wasmerio/wasmer/pull/3003) Remove RuntimeError::raise from public API
  - [#3000](https://github.com/wasmerio/wasmer/pull/3000) Allow debugging of EXC_BAD_INSTRUCTION on macOS
  - [#2946](https://github.com/wasmerio/wasmer/pull/2946) Removing dylib and staticlib engines in favor of a single Universal Engine
  - [#2996](https://github.com/wasmerio/wasmer/pull/2996) Migrated al examples to new Context API
  - [#2973](https://github.com/wasmerio/wasmer/pull/2973) Port C API to new Context API
  - [#2974](https://github.com/wasmerio/wasmer/pull/2974) Context api tests
  - [#2988](https://github.com/wasmerio/wasmer/pull/2988) Have make targets install-capi-lib,install-pkgconfig work without building the wasmer binary
  - [#2976](https://github.com/wasmerio/wasmer/pull/2976) Upgrade enumset minimum version to one that compiles
  - [#2969](https://github.com/wasmerio/wasmer/pull/2969) Port JS API to new Context API
  - [#2966](https://github.com/wasmerio/wasmer/pull/2966) Singlepass nopanic
  - [#2949](https://github.com/wasmerio/wasmer/pull/2949) Switch back to using custom LLVM builds on CI
  - [#2963](https://github.com/wasmerio/wasmer/pull/2963) Remove libxcb and libwayland dependencies from wasmer-cli release build
  - [#2957](https://github.com/wasmerio/wasmer/pull/2957) Enable multi-value handling in Singlepass compiler
  - [#2941](https://github.com/wasmerio/wasmer/pull/2941) Implementation of WASIX and a fully networking for Web Assembly
  - [#2947](https://github.com/wasmerio/wasmer/pull/2947) - Converted the WASI js test into a generic stdio test that works for…
  - [#2940](https://github.com/wasmerio/wasmer/pull/2940) Merge `wasmer3` back to `master` branch
  - [#2939](https://github.com/wasmerio/wasmer/pull/2939) Rename NativeFunc to TypedFunction

## Fixed

  - [#3045](https://github.com/wasmerio/wasmer/pull/3045) Fixed WASI fd_read syscall when reading multiple iovs and read is partial (for #2904)
  - [#2997](https://github.com/wasmerio/wasmer/pull/2997) Fix "run --invoke [function]" to behave the same as "run"
  - [#3027](https://github.com/wasmerio/wasmer/pull/3027) Fixed residual package-doc issues
  - [#3026](https://github.com/wasmerio/wasmer/pull/3026) test-js.yaml: fix typo
  - [#3017](https://github.com/wasmerio/wasmer/pull/3017) Fixed translation in README.md
  - [#3001](https://github.com/wasmerio/wasmer/pull/3001) Fix context capi ci errors
  - [#2967](https://github.com/wasmerio/wasmer/pull/2967) Fix singlepass on arm64 that was trying to emit a sub opcode with a constant as destination (for #2959)
  - [#2954](https://github.com/wasmerio/wasmer/pull/2954) Some fixes to x86_64 Singlepass compiler, when using atomics
  - [#2950](https://github.com/wasmerio/wasmer/pull/2950) compiler-cranelift: Fix typo in enum variant
  - [#2948](https://github.com/wasmerio/wasmer/pull/2948) Fix regression on gen_import_call_trampoline_arm64()
  - [#2943](https://github.com/wasmerio/wasmer/pull/2943) Fix build error on some archs by using c_char instead of i8
  - [#2944](https://github.com/wasmerio/wasmer/pull/2944) Fix duplicate entries in the CHANGELOG
  - [#2942](https://github.com/wasmerio/wasmer/pull/2942) Fix clippy lints

## 2.3.0 - 2022/06/06

### Added
- [#2862](https://github.com/wasmerio/wasmer/pull/2862) Added CI builds for linux-aarch64 target.
- [#2811](https://github.com/wasmerio/wasmer/pull/2811) Added support for EH Frames in singlepass
- [#2851](https://github.com/wasmerio/wasmer/pull/2851) Allow Wasmer to compile to Wasm/WASI

### Changed
- [#2807](https://github.com/wasmerio/wasmer/pull/2807) Run Wasm code in a separate stack
- [#2802](https://github.com/wasmerio/wasmer/pull/2802) Support Dylib engine with Singlepass
- [#2836](https://github.com/wasmerio/wasmer/pull/2836) Improve TrapInformation data stored at runtime
- [#2864](https://github.com/wasmerio/wasmer/pull/2864) `wasmer-cli`: remove wasi-experimental-io-devices from default builds
- [#2933](https://github.com/wasmerio/wasmer/pull/2933) Rename NativeFunc to TypedFunction.

### Fixed
- [#2829](https://github.com/wasmerio/wasmer/pull/2829) Improve error message oriented from JS object.
- [#2828](https://github.com/wasmerio/wasmer/pull/2828) Fix JsImportObject resolver.
- [#2872](https://github.com/wasmerio/wasmer/pull/2872) Fix `WasmerEnv` finalizer
- [#2821](https://github.com/wasmerio/wasmer/pull/2821) Opt in `sys` feature

## 2.2.1 - 2022/03/15

### Fixed
- [#2812](https://github.com/wasmerio/wasmer/pull/2812) Fixed another panic due to incorrect drop ordering.

## 2.2.0 - 2022/02/28

### Added
- [#2775](https://github.com/wasmerio/wasmer/pull/2775) Added support for SSE 4.2 in the Singlepass compiler as an alternative to AVX.
- [#2805](https://github.com/wasmerio/wasmer/pull/2805) Enabled WASI experimental I/O devices by default in releases.

### Fixed
- [#2795](https://github.com/wasmerio/wasmer/pull/2795) Fixed a bug in the Singlepass compiler introduced in #2775.
- [#2806](https://github.com/wasmerio/wasmer/pull/2806) Fixed a panic due to incorrect drop ordering of `Module` fields.

## 2.2.0-rc2 - 2022/02/15

### Fixed
- [#2778](https://github.com/wasmerio/wasmer/pull/2778) Fixed f32_load/f64_load in Singlepass. Also fixed issues with out-of-range conditional branches.
- [#2786](https://github.com/wasmerio/wasmer/pull/2786) Fixed a potential integer overflow in WasmPtr memory access methods.
- [#2787](https://github.com/wasmerio/wasmer/pull/2787) Fixed a codegen regression in the Singlepass compiler due to non-determinism of `HashSet` iteration.

## 2.2.0-rc1 - 2022/01/28

### Added
- [#2750](https://github.com/wasmerio/wasmer/pull/2750) Added Aarch64 support to Singlepass (both Linux and macOS).
- [#2753](https://github.com/wasmerio/wasmer/pull/2753) Re-add "dylib" to the list of default features.

### Changed
- [#2747](https://github.com/wasmerio/wasmer/pull/2747) Use a standard header for metadata in all serialized modules.
- [#2759](https://github.com/wasmerio/wasmer/pull/2759) Use exact version for Wasmer crate dependencies.

### Fixed
- [#2769](https://github.com/wasmerio/wasmer/pull/2769) Fixed deadlock in emscripten dynamic calls.
- [#2742](https://github.com/wasmerio/wasmer/pull/2742) Fixed WASMER_METADATA alignment in the dylib engine.
- [#2746](https://github.com/wasmerio/wasmer/pull/2746) Fixed invoking `wasmer binfmt register` from `$PATH`.
- [#2748](https://github.com/wasmerio/wasmer/pull/2748) Use trampolines for all libcalls in engine-universal and engine-dylib.
- [#2766](https://github.com/wasmerio/wasmer/pull/2766) Remove an attempt to reserve a GPR when no GPR clobbering is occurring.
- [#2768](https://github.com/wasmerio/wasmer/pull/2768) Fixed serialization of FrameInfo on Dylib engine.

## 2.1.1 - 2021/12/20

### Added
- [#2726](https://github.com/wasmerio/wasmer/pull/2726) Added `externs_vec` method to `ImportObject`.
- [#2724](https://github.com/wasmerio/wasmer/pull/2724) Added access to the raw `Instance` JS object in Wsasmer-js.

### CHanged
- [#2711](https://github.com/wasmerio/wasmer/pull/2711) Make C-API and Wasi dependencies more lean
- [#2706](https://github.com/wasmerio/wasmer/pull/2706) Refactored the Singlepass compiler in preparation for AArch64 support (no user visible changes).
### Fixed
- [#2717](https://github.com/wasmerio/wasmer/pull/2717) Allow `Exports` to be modified after being cloned.
- [#2719](https://github.com/wasmerio/wasmer/pull/2719) Fixed `wasm_importtype_new`'s Rust signature to not assume boxed vectors.
- [#2723](https://github.com/wasmerio/wasmer/pull/2723) Fixed a bug in parameter passing in the Singlepass compiler.
- [#2768](https://github.com/wasmerio/wasmer/pull/2768) Fixed issue with Frame Info on dylib engine.

## 2.1.0 - 2021/11/30

### Added
- [#2574](https://github.com/wasmerio/wasmer/pull/2574) Added Windows support to Singlepass.
- [#2535](https://github.com/wasmerio/wasmer/pull/2435) Added iOS support for Wasmer. This relies on the `dylib-engine`.
- [#2460](https://github.com/wasmerio/wasmer/pull/2460) Wasmer can now compile to Javascript via `wasm-bindgen`. Use the `js-default` (and no default features) feature to try it!.
- [#2491](https://github.com/wasmerio/wasmer/pull/2491) Added support for WASI to Wasmer-js.
- [#2436](https://github.com/wasmerio/wasmer/pull/2436) Added the x86-32 bit variant support to LLVM compiler.
- [#2499](https://github.com/wasmerio/wasmer/pull/2499) Added a subcommand to linux wasmer-cli to register wasmer with binfmt_misc
- [#2511](https://github.com/wasmerio/wasmer/pull/2511) Added support for calling dynamic functions defined on the host
- [#2491](https://github.com/wasmerio/wasmer/pull/2491) Added support for WASI in Wasmer-js
- [#2592](https://github.com/wasmerio/wasmer/pull/2592) Added `ImportObject::get_namespace_exports` to allow modifying the contents of an existing namespace in an `ImportObject`.
- [#2694](https://github.com/wasmerio/wasmer/pull/2694) wasmer-js: Allow an `ImportObject` to be extended with a JS object.
- [#2698](https://github.com/wasmerio/wasmer/pull/2698) Provide WASI imports when invoking an explicit export from the CLI.
- [#2701](https://github.com/wasmerio/wasmer/pull/2701) Improved VFS API for usage from JS

### Changed
- [#2460](https://github.com/wasmerio/wasmer/pull/2460) **breaking change** `wasmer` API usage with `no-default-features` requires now the `sys` feature to preserve old behavior.
- [#2476](https://github.com/wasmerio/wasmer/pull/2476) Removed unncessary abstraction `ModuleInfoTranslate` from `wasmer-compiler`.
- [#2442](https://github.com/wasmerio/wasmer/pull/2442) **breaking change** Improved `WasmPtr`, added `WasmCell` for host/guest interaction. `WasmPtr::deref` will now return `WasmCell<'a, T>` instead of `&'a Cell<T>`, `WasmPtr::deref_mut` is now deleted from the API.
- [#2427](https://github.com/wasmerio/wasmer/pull/2427) Update `loupe` to 0.1.3.
- [#2685](https://github.com/wasmerio/wasmer/pull/2685) The minimum LLVM version for the LLVM compiler is now 12. LLVM 13 is used by default.
- [#2569](https://github.com/wasmerio/wasmer/pull/2569) Add `Send` and `Sync` to uses of the `LikeNamespace` trait object.
- [#2692](https://github.com/wasmerio/wasmer/pull/2692) Made module serialization deterministic.
- [#2693](https://github.com/wasmerio/wasmer/pull/2693) Validate CPU features when loading a deserialized module.

### Fixed
- [#2599](https://github.com/wasmerio/wasmer/pull/2599) Fixed Universal engine for Linux/Aarch64 target.
- [#2587](https://github.com/wasmerio/wasmer/pull/2587) Fixed deriving `WasmerEnv` when aliasing `Result`.
- [#2518](https://github.com/wasmerio/wasmer/pull/2518) Remove temporary file used to creating an artifact when creating a Dylib engine artifact.
- [#2494](https://github.com/wasmerio/wasmer/pull/2494) Fixed `WasmerEnv` access when using `call_indirect` with the Singlepass compiler.
- [#2479](https://github.com/wasmerio/wasmer/pull/2479) Improved `wasmer validate` error message on non-wasm inputs.
- [#2454](https://github.com/wasmerio/wasmer/issues/2454) Won't set `WASMER_CACHE_DIR` for Windows.
- [#2426](https://github.com/wasmerio/wasmer/pull/2426) Fix the `wax` script generation.
- [#2635](https://github.com/wasmerio/wasmer/pull/2635) Fix cross-compilation for singlepass.
- [#2672](https://github.com/wasmerio/wasmer/pull/2672) Use `ENOENT` instead of `EINVAL` in some WASI syscalls for a non-existent file
- [#2547](https://github.com/wasmerio/wasmer/pull/2547) Delete temporary files created by the dylib engine.
- [#2548](https://github.com/wasmerio/wasmer/pull/2548) Fix stack probing on x86_64 linux with the cranelift compiler.
- [#2557](https://github.com/wasmerio/wasmer/pull/2557) [#2559](https://github.com/wasmerio/wasmer/pull/2559) Fix WASI dir path renaming.
- [#2560](https://github.com/wasmerio/wasmer/pull/2560) Fix signal handling on M1 MacOS.
- [#2474](https://github.com/wasmerio/wasmer/pull/2474) Fix permissions on `WASMER_CACHE_DIR` on Windows.
- [#2528](https://github.com/wasmerio/wasmer/pull/2528) [#2525](https://github.com/wasmerio/wasmer/pull/2525) [#2523](https://github.com/wasmerio/wasmer/pull/2523) [#2522](https://github.com/wasmerio/wasmer/pull/2522) [#2545](https://github.com/wasmerio/wasmer/pull/2545) [#2550](https://github.com/wasmerio/wasmer/pull/2550) [#2551](https://github.com/wasmerio/wasmer/pull/2551)  Fix various bugs in the new VFS implementation.
- [#2552](https://github.com/wasmerio/wasmer/pull/2552) Fix stack guard handling on Windows.
- [#2585](https://github.com/wasmerio/wasmer/pull/2585) Fix build with 64-bit MinGW toolchain.
- [#2587](https://github.com/wasmerio/wasmer/pull/2587) Fix absolute import of `Result` in derive.
- [#2599](https://github.com/wasmerio/wasmer/pull/2599) Fix AArch64 support in the LLVM compiler.
- [#2655](https://github.com/wasmerio/wasmer/pull/2655) Fix argument parsing of `--dir` and `--mapdir`.
- [#2666](https://github.com/wasmerio/wasmer/pull/2666) Fix performance on Windows by using static memories by default.
- [#2667](https://github.com/wasmerio/wasmer/pull/2667) Fix error code for path_rename of a non-existant file
- [#2672](https://github.com/wasmerio/wasmer/pull/2672) Fix error code returned by some wasi fs syscalls for a non-existent file
- [#2673](https://github.com/wasmerio/wasmer/pull/2673) Fix BrTable codegen on the LLVM compiler
- [#2674](https://github.com/wasmerio/wasmer/pull/2674) Add missing `__WASI_RIGHT_FD_DATASYNC` for preopened directories
- [#2677](https://github.com/wasmerio/wasmer/pull/2677) Support 32-bit memories with 65536 pages
- [#2681](https://github.com/wasmerio/wasmer/pull/2681) Fix slow compilation in singlepass by using dynasm's `VecAssembler`.
- [#2690](https://github.com/wasmerio/wasmer/pull/2690) Fix memory leak when obtaining the stack bounds of a thread
- [#2699](https://github.com/wasmerio/wasmer/pull/2699) Partially fix unbounded memory leak from the FuncDataRegistry

## 2.0.0 - 2021/06/16

### Added
- [#2411](https://github.com/wasmerio/wasmer/pull/2411) Extract types from `wasi` to a new `wasi-types` crate.
- [#2390](https://github.com/wasmerio/wasmer/pull/2390) Make `wasmer-vm` to compile on Windows 32bits.
- [#2402](https://github.com/wasmerio/wasmer/pull/2402) Add more examples and more doctests for `wasmer-middlewares`.

### Changed
- [#2399](https://github.com/wasmerio/wasmer/pull/2399) Add the Dart integration in the `README.md`.

### Fixed
- [#2386](https://github.com/wasmerio/wasmer/pull/2386) Handle properly when a module has no exported functions in the CLI.

## 2.0.0-rc2 - 2021/06/03

### Fixed
- [#2383](https://github.com/wasmerio/wasmer/pull/2383) Fix bugs in the Wasmer CLI tool with the way `--version` and the name of the CLI tool itself were printed.

## 2.0.0-rc1 - 2021/06/02

### Added
- [#2348](https://github.com/wasmerio/wasmer/pull/2348) Make Wasmer available on `aarch64-linux-android`.
- [#2315](https://github.com/wasmerio/wasmer/pull/2315) Make the Cranelift compiler working with the Native engine.
- [#2306](https://github.com/wasmerio/wasmer/pull/2306) Add support for the latest version of the Wasm SIMD proposal to compiler LLVM.
- [#2296](https://github.com/wasmerio/wasmer/pull/2296) Add support for the bulk memory proposal in compiler Singlepass and compiler LLVM.
- [#2291](https://github.com/wasmerio/wasmer/pull/2291) Type check tables when importing.
- [#2262](https://github.com/wasmerio/wasmer/pull/2262) Make parallelism optional for the Singlepass compiler.
- [#2249](https://github.com/wasmerio/wasmer/pull/2249) Make Cranelift unwind feature optional.
- [#2208](https://github.com/wasmerio/wasmer/pull/2208) Add a new CHANGELOG.md specific to our C API to make it easier for users primarily consuming our C API to keep up to date with changes that affect them.
- [#2154](https://github.com/wasmerio/wasmer/pull/2154) Implement Reference Types in the LLVM compiler.
- [#2003](https://github.com/wasmerio/wasmer/pull/2003) Wasmer works with musl, and is built, tested and packaged for musl.
- [#2250](https://github.com/wasmerio/wasmer/pull/2250) Use `rkyv` for the JIT/Universal engine.
- [#2190](https://github.com/wasmerio/wasmer/pull/2190) Use `rkyv` to read native `Module` artifact.
- [#2186](https://github.com/wasmerio/wasmer/pull/2186) Update and improve the Fuzz Testing infrastructure.
- [#2161](https://github.com/wasmerio/wasmer/pull/2161) Make NaN canonicalization configurable.
- [#2116](https://github.com/wasmerio/wasmer/pull/2116) Add a package for Windows that is not an installer, but all the `lib` and `include` files as for macOS and Linux.
- [#2123](https://github.com/wasmerio/wasmer/pull/2123) Use `ENABLE_{{compiler_name}}=(0|1)` to resp. force to disable or enable a compiler when running the `Makefile`, e.g. `ENABLE_LLVM=1 make build-wasmer`.
- [#2123](https://github.com/wasmerio/wasmer/pull/2123) `libwasmer` comes with all available compilers per target instead of Cranelift only.
- [#2135](https://github.com/wasmerio/wasmer/pull/2135) [Documentation](./PACKAGING.md) for Linux distribution maintainers
- [#2104](https://github.com/wasmerio/wasmer/pull/2104) Update WAsm core spectests and wasmparser.

### Changed
- [#2369](https://github.com/wasmerio/wasmer/pull/2369) Remove the deprecated `--backend` option in the CLI.
- [#2368](https://github.com/wasmerio/wasmer/pull/2368) Remove the deprecated code in the `wasmer-wasi` crate.
- [#2367](https://github.com/wasmerio/wasmer/pull/2367) Remove the `deprecated` features and associated code in the `wasmer` crate.
- [#2366](https://github.com/wasmerio/wasmer/pull/2366) Remove the deprecated crates.
- [#2364](https://github.com/wasmerio/wasmer/pull/2364) Rename `wasmer-engine-object-file` to `wasmer-engine-staticlib`.
- [#2356](https://github.com/wasmerio/wasmer/pull/2356) Rename `wasmer-engine-native` to `wasmer-engine-dylib`.
- [#2340](https://github.com/wasmerio/wasmer/pull/2340) Rename `wasmer-engine-jit` to `wasmer-engine-universal`.
- [#2307](https://github.com/wasmerio/wasmer/pull/2307) Update Cranelift, implement low hanging fruit SIMD opcodes.
- [#2305](https://github.com/wasmerio/wasmer/pull/2305) Clean up and improve the trap API, more deterministic errors etc.
- [#2299](https://github.com/wasmerio/wasmer/pull/2299) Unused trap codes (due to Wasm spec changes), `HeapSetterOutOfBounds` and `TableSetterOutOfBounds` were removed from `wasmer_vm::TrapCode` and the numbering of the remaining variants has been adjusted.
- [#2293](https://github.com/wasmerio/wasmer/pull/2293) The `Memory::ty` trait method now returns `MemoryType` by value. `wasmer_vm::LinearMemory` now recomputes `MemoryType`'s `minimum` field when accessing its type. This behavior is what's expected by the latest spectests. `wasmer::Memory::ty` has also been updated to follow suit, it now returns `MemoryType` by value.
- [#2286](https://github.com/wasmerio/wasmer/pull/2286) Replace the `goblin` crate by the `object` crate.
- [#2281](https://github.com/wasmerio/wasmer/pull/2281) Refactor the `wasmer_vm` crate to remove unnecessary structs, reuse data when available etc.
- [#2251](https://github.com/wasmerio/wasmer/pull/2251) Wasmer CLI will now execute WASI modules with multiple WASI namespaces in them by default. Use `--allow-multiple-wasi-versions` to suppress the warning and use `--deny-multiple-wasi-versions` to make it an error.
- [#2201](https://github.com/wasmerio/wasmer/pull/2201) Implement `loupe::MemoryUsage` for `wasmer::Instance`.
- [#2200](https://github.com/wasmerio/wasmer/pull/2200) Implement `loupe::MemoryUsage` for `wasmer::Module`.
- [#2199](https://github.com/wasmerio/wasmer/pull/2199) Implement `loupe::MemoryUsage` for `wasmer::Store`.
- [#2195](https://github.com/wasmerio/wasmer/pull/2195) Remove dependency to `cranelift-entity`.
- [#2140](https://github.com/wasmerio/wasmer/pull/2140) Reduce the number of dependencies in the `wasmer.dll` shared library by statically compiling CRT.
- [#2113](https://github.com/wasmerio/wasmer/pull/2113) Bump minimum supported Rust version to 1.49
- [#2144](https://github.com/wasmerio/wasmer/pull/2144) Bump cranelift version to 0.70
- [#2149](https://github.com/wasmerio/wasmer/pull/2144) `wasmer-engine-native` looks for clang-11 instead of clang-10.
- [#2157](https://github.com/wasmerio/wasmer/pull/2157) Simplify the code behind `WasmPtr`

### Fixed
- [#2397](https://github.com/wasmerio/wasmer/pull/2397) Fix WASI rename temporary file issue.
- [#2391](https://github.com/wasmerio/wasmer/pull/2391) Fix Singlepass emit bug, [#2347](https://github.com/wasmerio/wasmer/issues/2347) and [#2159](https://github.com/wasmerio/wasmer/issues/2159)
- [#2327](https://github.com/wasmerio/wasmer/pull/2327) Fix memory leak preventing internal instance memory from being freed when a WasmerEnv contained an exported extern (e.g. Memory, etc.).
- [#2247](https://github.com/wasmerio/wasmer/pull/2247) Internal WasiFS logic updated to be closer to what WASI libc does when finding a preopened fd for a path.
- [#2241](https://github.com/wasmerio/wasmer/pull/2241) Fix Undefined Behavior in setting memory in emscripten `EmEnv`.
- [#2224](https://github.com/wasmerio/wasmer/pull/2224) Enable SIMD based on actual Wasm features in the Cranelift compiler.
- [#2217](https://github.com/wasmerio/wasmer/pull/2217) Fix bug in `i64.rotr X 0` in the LLVM compiler.
- [#2290](https://github.com/wasmerio/wasmer/pull/2290) Handle Wasm modules with no imports in the CLI.
- [#2108](https://github.com/wasmerio/wasmer/pull/2108) The Object Native Engine generates code that now compiles correctly with C++.
- [#2125](https://github.com/wasmerio/wasmer/pull/2125) Fix RUSTSEC-2021-0023.
- [#2155](https://github.com/wasmerio/wasmer/pull/2155) Fix the implementation of shift and rotate in the LLVM compiler.
- [#2101](https://github.com/wasmerio/wasmer/pull/2101) cflags emitted by `wasmer config --pkg-config` are now correct.

## 1.0.2 - 2021-02-04

### Added
- [#2053](https://github.com/wasmerio/wasmer/pull/2053) Implement the non-standard `wasi_get_unordered_imports` function in the C API.
- [#2072](https://github.com/wasmerio/wasmer/pull/2072) Add `wasm_config_set_target`, along with `wasm_target_t`, `wasm_triple_t` and `wasm_cpu_features_t` in the unstable C API.
- [#2059](https://github.com/wasmerio/wasmer/pull/2059) Ability to capture `stdout` and `stderr` with WASI in the C API.
- [#2040](https://github.com/wasmerio/wasmer/pull/2040) Add `InstanceHandle::vmoffsets` to expose the offsets of the `vmctx` region.
- [#2026](https://github.com/wasmerio/wasmer/pull/2026) Expose trap code of a `RuntimeError`, if it's a `Trap`.
- [#2054](https://github.com/wasmerio/wasmer/pull/2054) Add `wasm_config_delete` to the Wasm C API.
- [#2072](https://github.com/wasmerio/wasmer/pull/2072) Added cross-compilation to Wasm C API.

### Changed
- [#2085](https://github.com/wasmerio/wasmer/pull/2085) Update to latest inkwell and LLVM 11.
- [#2037](https://github.com/wasmerio/wasmer/pull/2037) Improved parallelism of LLVM with the Native/Object engine
- [#2012](https://github.com/wasmerio/wasmer/pull/2012) Refactor Singlepass init stack assembly (more performant now)
- [#2036](https://github.com/wasmerio/wasmer/pull/2036) Optimize memory allocated for Function type definitions
- [#2083](https://github.com/wasmerio/wasmer/pull/2083) Mark `wasi_env_set_instance` and `wasi_env_set_memory` as deprecated. You may simply remove the calls with no side-effect.
- [#2056](https://github.com/wasmerio/wasmer/pull/2056) Change back to depend on the `enumset` crate instead of `wasmer_enumset`

### Fixed
- [#2066](https://github.com/wasmerio/wasmer/pull/2066) Include 'extern "C"' in our C headers when included by C++ code.
- [#2090](https://github.com/wasmerio/wasmer/pull/2090) `wasi_env_t` needs to be freed with `wasi_env_delete` in the C API.
- [#2084](https://github.com/wasmerio/wasmer/pull/2084) Avoid calling the function environment finalizer more than once when the environment has been cloned in the C API.
- [#2069](https://github.com/wasmerio/wasmer/pull/2069) Use the new documentation for `include/README.md` in the Wasmer package.
- [#2042](https://github.com/wasmerio/wasmer/pull/2042) Parse more exotic environment variables in `wasmer run`.
- [#2041](https://github.com/wasmerio/wasmer/pull/2041) Documentation diagrams now have a solid white background rather than a transparent background.
- [#2070](https://github.com/wasmerio/wasmer/pull/2070) Do not drain the entire captured stream at first read with `wasi_env_read_stdout` or `_stderr` in the C API.
- [#2058](https://github.com/wasmerio/wasmer/pull/2058) Expose WASI versions to C correctly.
- [#2044](https://github.com/wasmerio/wasmer/pull/2044) Do not build C headers on docs.rs.

## 1.0.1 - 2021-01-12

This release includes a breaking change in the API (changing the trait `enumset::EnumsetType` to `wasmer_enumset::EnumSetType` and changing `enumset::EnumSet` in signatures to `wasmer_enumset::EnumSet` to work around a breaking change introduced by `syn`) but is being released as a minor version because `1.0.0` is also in a broken state due to a breaking change introduced by `syn` which affects `enumset` and thus `wasmer`.

This change is unlikely to affect any users of `wasmer`, but if it does please change uses of the `enumset` crate to the `wasmer_enumset` crate where possible.

### Added
- [#2010](https://github.com/wasmerio/wasmer/pull/2010) A new, experimental, minified build of `wasmer` called `wasmer-headless` will now be included with releases. `wasmer-headless` is the `wasmer` VM without any compilers attached, so it can only run precompiled Wasm modules.
- [#2005](https://github.com/wasmerio/wasmer/pull/2005) Added the arguments `alias` and `optional` to `WasmerEnv` derive's `export` attribute.

### Changed
- [#2006](https://github.com/wasmerio/wasmer/pull/2006) Use `wasmer_enumset`, a fork of the `enumset` crate to work around a breaking change in `syn`
- [#1985](https://github.com/wasmerio/wasmer/pull/1985) Bump minimum supported Rust version to 1.48

### Fixed
- [#2007](https://github.com/wasmerio/wasmer/pull/2007) Fix packaging of wapm on Windows
- [#2005](https://github.com/wasmerio/wasmer/pull/2005) Emscripten is now working again.

## 1.0.0 - 2021-01-05

### Added

- [#1969](https://github.com/wasmerio/wasmer/pull/1969) Added D integration to the README

### Changed
- [#1979](https://github.com/wasmerio/wasmer/pull/1979) `WasmPtr::get_utf8_string` was renamed to `WasmPtr::get_utf8_str` and made `unsafe`.

### Fixed
- [#1979](https://github.com/wasmerio/wasmer/pull/1979) `WasmPtr::get_utf8_string` now returns a `String`, fixing a soundness issue in certain circumstances. The old functionality is available under a new `unsafe` function, `WasmPtr::get_utf8_str`.

## 1.0.0-rc1 - 2020-12-23

### Added

* [#1894](https://github.com/wasmerio/wasmer/pull/1894) Added exports `wasmer::{CraneliftOptLevel, LLVMOptLevel}` to allow using `Cranelift::opt_level` and `LLVM::opt_level` directly via the `wasmer` crate

### Changed

* [#1941](https://github.com/wasmerio/wasmer/pull/1941) Turn `get_remaining_points`/`set_remaining_points` of the `Metering` middleware into free functions to allow using them in an ahead-of-time compilation setup
* [#1955](https://github.com/wasmerio/wasmer/pull/1955) Set `jit` as a default feature of the `wasmer-wasm-c-api` crate
* [#1944](https://github.com/wasmerio/wasmer/pull/1944) Require `WasmerEnv` to be `Send + Sync` even in dynamic functions.
* [#1963](https://github.com/wasmerio/wasmer/pull/1963) Removed `to_wasm_error` in favour of `impl From<BinaryReaderError> for WasmError`
* [#1962](https://github.com/wasmerio/wasmer/pull/1962) Replace `wasmparser::Result<()>` with `Result<(), MiddlewareError>` in middleware, allowing implementors to return errors in `FunctionMiddleware::feed`

### Fixed

- [#1949](https://github.com/wasmerio/wasmer/pull/1949) `wasm_<type>_vec_delete` functions no longer crash when the given vector is uninitialized, in the Wasmer C API
- [#1949](https://github.com/wasmerio/wasmer/pull/1949) The `wasm_frame_vec_t`, `wasm_functype_vec_t`, `wasm_globaltype_vec_t`, `wasm_memorytype_vec_t`, and `wasm_tabletype_vec_t` are now boxed vectors in the Wasmer C API

## 1.0.0-beta2 - 2020-12-16

### Added

* [#1916](https://github.com/wasmerio/wasmer/pull/1916) Add the `WASMER_VERSION*` constants with the `wasmer_version*` functions in the Wasmer C API
* [#1867](https://github.com/wasmerio/wasmer/pull/1867) Added `Metering::get_remaining_points` and `Metering::set_remaining_points`
* [#1881](https://github.com/wasmerio/wasmer/pull/1881) Added `UnsupportedTarget` error to `CompileError`
* [#1908](https://github.com/wasmerio/wasmer/pull/1908) Implemented `TryFrom<Value<T>>` for `i32`/`u32`/`i64`/`u64`/`f32`/`f64`
* [#1927](https://github.com/wasmerio/wasmer/pull/1927) Added mmap support in `Engine::deserialize_from_file` to speed up artifact loading
* [#1911](https://github.com/wasmerio/wasmer/pull/1911) Generalized signature type in `Function::new` and `Function::new_with_env` to accept owned and reference `FunctionType` as well as array pairs. This allows users to define signatures as constants. Implemented `From<([Type; $N], [Type; $M])>` for `FunctionType` to support this.

### Changed

- [#1865](https://github.com/wasmerio/wasmer/pull/1865) Require that implementors of `WasmerEnv` also implement `Send`, `Sync`, and `Clone`.
- [#1851](https://github.com/wasmerio/wasmer/pull/1851) Improve test suite and documentation of the Wasmer C API
- [#1874](https://github.com/wasmerio/wasmer/pull/1874) Set `CompilerConfig` to be owned (following wasm-c-api)
- [#1880](https://github.com/wasmerio/wasmer/pull/1880) Remove cmake dependency for tests
- [#1924](https://github.com/wasmerio/wasmer/pull/1924) Rename reference implementation `wasmer::Tunables` to `wasmer::BaseTunables`. Export trait `wasmer_engine::Tunables` as `wasmer::Tunables`.

### Fixed

- [#1865](https://github.com/wasmerio/wasmer/pull/1865) Fix memory leaks with host function environments.
- [#1870](https://github.com/wasmerio/wasmer/pull/1870) Fixed Trap instruction address maps in Singlepass
* [#1914](https://github.com/wasmerio/wasmer/pull/1914) Implemented `TryFrom<Bytes> for Pages` instead of `From<Bytes> for Pages` to properly handle overflow errors

## 1.0.0-beta1 - 2020-12-01

### Added

- [#1839](https://github.com/wasmerio/wasmer/pull/1839) Added support for Metering Middleware
- [#1837](https://github.com/wasmerio/wasmer/pull/1837) It is now possible to use exports of an `Instance` even after the `Instance` has been freed
- [#1831](https://github.com/wasmerio/wasmer/pull/1831) Added support for Apple Silicon chips (`arm64-apple-darwin`)
- [#1739](https://github.com/wasmerio/wasmer/pull/1739) Improved function environment setup via `WasmerEnv` proc macro.
- [#1649](https://github.com/wasmerio/wasmer/pull/1649) Add outline of migration to 1.0.0 docs.

### Changed

- [#1739](https://github.com/wasmerio/wasmer/pull/1739) Environments passed to host function- must now implement the `WasmerEnv` trait. You can implement it on your existing type with `#[derive(WasmerEnv)]`.
- [#1838](https://github.com/wasmerio/wasmer/pull/1838) Deprecate `WasiEnv::state_mut`: prefer `WasiEnv::state` instead.
- [#1663](https://github.com/wasmerio/wasmer/pull/1663) Function environments passed to host functions now must be passed by `&` instead of `&mut`. This is a breaking change. This change fixes a race condition when a host function is called from multiple threads. If you need mutability in your environment, consider using `std::sync::Mutex` or other synchronization primitives.
- [#1830](https://github.com/wasmerio/wasmer/pull/1830) Minimum supported Rust version bumped to 1.47.0
- [#1810](https://github.com/wasmerio/wasmer/pull/1810) Make the `state` field of `WasiEnv` public

### Fixed

- [#1857](https://github.com/wasmerio/wasmer/pull/1857) Fix dynamic function with new Environment API
- [#1855](https://github.com/wasmerio/wasmer/pull/1855) Fix memory leak when using `wat2wasm` in the C API, the function now takes its output parameter by pointer rather than returning an allocated `wasm_byte_vec_t`.
- [#1841](https://github.com/wasmerio/wasmer/pull/1841) We will now panic when attempting to use a native function with a captured env as a host function. Previously this would silently do the wrong thing. See [#1840](https://github.com/wasmerio/wasmer/pull/1840) for info about Wasmer's support of closures as host functions.
- [#1764](https://github.com/wasmerio/wasmer/pull/1764) Fix bug in WASI `path_rename` allowing renamed files to be 1 directory below a preopened directory.

## 1.0.0-alpha5 - 2020-11-06

### Added

- [#1761](https://github.com/wasmerio/wasmer/pull/1761) Implement the `wasm_trap_t**` argument of `wasm_instance_new` in the Wasm C API.
- [#1687](https://github.com/wasmerio/wasmer/pull/1687) Add basic table example; fix ownership of local memory and local table metadata in the VM.
- [#1751](https://github.com/wasmerio/wasmer/pull/1751) Implement `wasm_trap_t` inside a function declared with `wasm_func_new_with_env` in the Wasm C API.
- [#1741](https://github.com/wasmerio/wasmer/pull/1741) Implement `wasm_memory_type` in the Wasm C API.
- [#1736](https://github.com/wasmerio/wasmer/pull/1736) Implement `wasm_global_type` in the Wasm C API.
- [#1699](https://github.com/wasmerio/wasmer/pull/1699) Update `wasm.h` to its latest version.
- [#1685](https://github.com/wasmerio/wasmer/pull/1685) Implement `wasm_exporttype_delete` in the Wasm C API.
- [#1725](https://github.com/wasmerio/wasmer/pull/1725) Implement `wasm_func_type` in the Wasm C API.
- [#1715](https://github.com/wasmerio/wasmer/pull/1715) Register errors from `wasm_module_serialize` in the Wasm C API.
- [#1709](https://github.com/wasmerio/wasmer/pull/1709) Implement `wasm_module_name` and `wasm_module_set_name` in the Wasm(er) C API.
- [#1700](https://github.com/wasmerio/wasmer/pull/1700) Implement `wasm_externtype_copy` in the Wasm C API.
- [#1785](https://github.com/wasmerio/wasmer/pull/1785) Add more examples on the Rust API.
- [#1783](https://github.com/wasmerio/wasmer/pull/1783) Handle initialized but empty results in `wasm_func_call` in the Wasm C API.
- [#1780](https://github.com/wasmerio/wasmer/pull/1780) Implement new SIMD zero-extend loads in compiler-llvm.
- [#1754](https://github.com/wasmerio/wasmer/pull/1754) Implement aarch64 ABI for compiler-llvm.
- [#1693](https://github.com/wasmerio/wasmer/pull/1693) Add `wasmer create-exe` subcommand.

### Changed

- [#1772](https://github.com/wasmerio/wasmer/pull/1772) Remove lifetime parameter from `NativeFunc`.
- [#1762](https://github.com/wasmerio/wasmer/pull/1762) Allow the `=` sign in a WASI environment variable value.
- [#1710](https://github.com/wasmerio/wasmer/pull/1710) Memory for function call trampolines is now owned by the Artifact.
- [#1781](https://github.com/wasmerio/wasmer/pull/1781) Cranelift upgrade to 0.67.
- [#1777](https://github.com/wasmerio/wasmer/pull/1777) Wasmparser update to 0.65.
- [#1775](https://github.com/wasmerio/wasmer/pull/1775) Improve LimitingTunables implementation.
- [#1720](https://github.com/wasmerio/wasmer/pull/1720) Autodetect llvm regardless of architecture.

### Fixed

- [#1718](https://github.com/wasmerio/wasmer/pull/1718) Fix panic in the API in some situations when the memory's min bound was greater than the memory's max bound.
- [#1731](https://github.com/wasmerio/wasmer/pull/1731) In compiler-llvm always load before store, to trigger any traps before any bytes are written.

## 1.0.0-alpha4 - 2020-10-08

### Added
- [#1635](https://github.com/wasmerio/wasmer/pull/1635) Implement `wat2wasm` in the Wasm C API.
- [#1636](https://github.com/wasmerio/wasmer/pull/1636) Implement `wasm_module_validate` in the Wasm C API.
- [#1657](https://github.com/wasmerio/wasmer/pull/1657) Implement `wasm_trap_t` and `wasm_frame_t` for Wasm C API; add examples in Rust and C of exiting early with a host function.

### Fixed
- [#1690](https://github.com/wasmerio/wasmer/pull/1690) Fix `wasm_memorytype_limits` where `min` and `max` represents pages, not bytes. Additionally, fixes the max limit sentinel value.
- [#1671](https://github.com/wasmerio/wasmer/pull/1671) Fix probestack firing inappropriately, and sometimes over/under allocating stack.
- [#1660](https://github.com/wasmerio/wasmer/pull/1660) Fix issue preventing map-dir aliases starting with `/` from working properly.
- [#1624](https://github.com/wasmerio/wasmer/pull/1624) Add Value::I32/Value::I64 converters from unsigned ints.

### Changed
- [#1682](https://github.com/wasmerio/wasmer/pull/1682) Improve error reporting when making a memory with invalid settings.
- [#1691](https://github.com/wasmerio/wasmer/pull/1691) Bump minimum supported Rust version to 1.46.0
- [#1645](https://github.com/wasmerio/wasmer/pull/1645) Move the install script to https://github.com/wasmerio/wasmer-install

## 1.0.0-alpha3 - 2020-09-14

### Fixed

- [#1620](https://github.com/wasmerio/wasmer/pull/1620) Fix bug causing the Wapm binary to not be packaged with the release
- [#1619](https://github.com/wasmerio/wasmer/pull/1619) Improve error message in engine-native when C compiler is missing

## 1.0.0-alpha02.0 - 2020-09-11

### Added

- [#1566](https://github.com/wasmerio/wasmer/pull/1566) Add support for opening special Unix files to the WASI FS

### Fixed

- [#1602](https://github.com/wasmerio/wasmer/pull/1602) Fix panic when calling host functions with negative numbers in certain situations
- [#1590](https://github.com/wasmerio/wasmer/pull/1590) Fix soundness issue in API of vm::Global

## TODO: 1.0.0-alpha01.0

- Wasmer refactor lands

## 0.17.1 - 2020-06-24

### Changed
- [#1439](https://github.com/wasmerio/wasmer/pull/1439) Move `wasmer-interface-types` into its own repository

### Fixed

- [#1554](https://github.com/wasmerio/wasmer/pull/1554) Update supported stable Rust version to 1.45.2.
- [#1552](https://github.com/wasmerio/wasmer/pull/1552) Disable `sigint` handler by default.

## 0.17.0 - 2020-05-11

### Added
- [#1331](https://github.com/wasmerio/wasmer/pull/1331) Implement the `record` type and instrutions for WIT
- [#1345](https://github.com/wasmerio/wasmer/pull/1345) Adding ARM testing in Azure Pipelines
- [#1329](https://github.com/wasmerio/wasmer/pull/1329) New numbers and strings instructions for WIT
- [#1285](https://github.com/wasmerio/wasmer/pull/1285) Greatly improve errors in `wasmer-interface-types`
- [#1303](https://github.com/wasmerio/wasmer/pull/1303) NaN canonicalization for singlepass backend.
- [#1313](https://github.com/wasmerio/wasmer/pull/1313) Add new high-level public API through `wasmer` crate. Includes many updates including:
  - Minor improvement: `imports!` macro now handles no trailing comma as well as a trailing comma in namespaces and between namespaces.
  - New methods on `Module`: `exports`, `imports`, and `custom_sections`.
  - New way to get exports from an instance with `let func_name: Func<i32, i64> = instance.exports.get("func_name");`.
  - Improved `Table` APIs including `set` which now allows setting functions directly.  TODO: update this more if `Table::get` gets made public in this PR
  - TODO: finish the list of changes here
- [#1305](https://github.com/wasmerio/wasmer/pull/1305) Handle panics from DynamicFunc.
- [#1300](https://github.com/wasmerio/wasmer/pull/1300) Add support for multiple versions of WASI tests: wasitests now test all versions of WASI.
- [#1292](https://github.com/wasmerio/wasmer/pull/1292) Experimental Support for Android (x86_64 and AArch64)

### Fixed
- [#1283](https://github.com/wasmerio/wasmer/pull/1283) Workaround for floating point arguments and return values in `DynamicFunc`s.

### Changed
- [#1401](https://github.com/wasmerio/wasmer/pull/1401) Make breaking change to `RuntimeError`: `RuntimeError` is now more explicit about its possible error values allowing for better insight into why a call into Wasm failed.
- [#1382](https://github.com/wasmerio/wasmer/pull/1382) Refactored test infranstructure (part 2)
- [#1380](https://github.com/wasmerio/wasmer/pull/1380) Refactored test infranstructure (part 1)
- [#1357](https://github.com/wasmerio/wasmer/pull/1357) Refactored bin commands into separate files
- [#1335](https://github.com/wasmerio/wasmer/pull/1335) Change mutability of `memory` to `const` in `wasmer_memory_data_length` in the C API
- [#1332](https://github.com/wasmerio/wasmer/pull/1332) Add option to `CompilerConfig` to force compiler IR verification off even when `debug_assertions` are enabled. This can be used to make debug builds faster, which may be important if you're creating a library that wraps Wasmer and depend on the speed of debug builds.
- [#1320](https://github.com/wasmerio/wasmer/pull/1320) Change `custom_sections` field in `ModuleInfo` to be more standards compliant by allowing multiple custom sections with the same name. To get the old behavior with the new API, you can add `.last().unwrap()` to accesses. For example, `module_info.custom_sections["custom_section_name"].last().unwrap()`.
- [#1301](https://github.com/wasmerio/wasmer/pull/1301) Update supported stable Rust version to 1.41.1.

## 0.16.2 - 2020-03-11

### Fixed

- [#1294](https://github.com/wasmerio/wasmer/pull/1294) Fix bug related to system calls in WASI that rely on reading from WasmPtrs as arrays of length 0. `WasmPtr` will now succeed on length 0 arrays again.

## 0.16.1 - 2020-03-11

### Fixed

- [#1291](https://github.com/wasmerio/wasmer/pull/1291) Fix installation packaging script to package the `wax` command.

## 0.16.0 - 2020-03-11

### Added
- [#1286](https://github.com/wasmerio/wasmer/pull/1286) Updated Windows Wasmer icons. Add wax
- [#1284](https://github.com/wasmerio/wasmer/pull/1284) Implement string and memory instructions in `wasmer-interface-types`

### Fixed
- [#1272](https://github.com/wasmerio/wasmer/pull/1272) Fix off-by-one error bug when accessing memory with a `WasmPtr` that contains the last valid byte of memory. Also changes the behavior of `WasmPtr<T, Array>` with a length of 0 and `WasmPtr<T>` where `std::mem::size_of::<T>()` is 0 to always return `None`

## 0.15.0 - 2020-03-04

- [#1263](https://github.com/wasmerio/wasmer/pull/1263) Changed the behavior of some WASI syscalls to now handle preopened directories more properly. Changed default `--debug` logging to only show Wasmer-related messages.
- [#1217](https://github.com/wasmerio/wasmer/pull/1217) Polymorphic host functions based on dynamic trampoline generation.
- [#1252](https://github.com/wasmerio/wasmer/pull/1252) Allow `/` in wasi `--mapdir` wasm path.
- [#1212](https://github.com/wasmerio/wasmer/pull/1212) Add support for GDB JIT debugging:
  - Add `--generate-debug-info` and `-g` flags to `wasmer run` to generate debug information during compilation. The debug info is passed via the GDB JIT interface to a debugger to allow source-level debugging of Wasm files. Currently only available on clif-backend.
  - Break public middleware APIs: there is now a `source_loc` parameter that should be passed through if applicable.
  - Break compiler trait methods such as `feed_local`, `feed_event` as well as `ModuleCodeGenerator::finalize`.

## 0.14.1 - 2020-02-24

- [#1245](https://github.com/wasmerio/wasmer/pull/1245) Use Ubuntu 16.04 in CI so that we use an earlier version of GLIBC.
- [#1234](https://github.com/wasmerio/wasmer/pull/1234) Check for unused excluded spectest failures.
- [#1232](https://github.com/wasmerio/wasmer/pull/1232) `wasmer-interface-types` has a WAT decoder.

## 0.14.0 - 2020-02-20

- [#1233](https://github.com/wasmerio/wasmer/pull/1233) Improved Wasmer C API release artifacts.
- [#1216](https://github.com/wasmerio/wasmer/pull/1216) `wasmer-interface-types` receives a binary encoder.
- [#1228](https://github.com/wasmerio/wasmer/pull/1228) Singlepass cleanup: Resolve several FIXMEs and remove protect_unix.
- [#1218](https://github.com/wasmerio/wasmer/pull/1218) Enable Cranelift verifier in debug mode. Fix bug with table indices being the wrong type.
- [#787](https://github.com/wasmerio/wasmer/pull/787) New crate `wasmer-interface-types` to implement WebAssembly Interface Types.
- [#1213](https://github.com/wasmerio/wasmer/pull/1213) Fixed WASI `fdstat` to detect `isatty` properly.
- [#1192](https://github.com/wasmerio/wasmer/pull/1192) Use `ExceptionCode` for error representation.
- [#1191](https://github.com/wasmerio/wasmer/pull/1191) Fix singlepass miscompilation on `Operator::CallIndirect`.
- [#1180](https://github.com/wasmerio/wasmer/pull/1180) Fix compilation for target `x86_64-unknown-linux-musl`.
- [#1170](https://github.com/wasmerio/wasmer/pull/1170) Improve the WasiFs builder API with convenience methods for overriding stdin, stdout, and stderr as well as a new sub-builder for controlling the permissions and properties of preopened directories.  Also breaks that implementations of `WasiFile` must be `Send` -- please file an issue if this change causes you any issues.
- [#1161](https://github.com/wasmerio/wasmer/pull/1161) Require imported functions to be `Send`. This is a breaking change that fixes a soundness issue in the API.
- [#1140](https://github.com/wasmerio/wasmer/pull/1140) Use [`blake3`](https://github.com/BLAKE3-team/BLAKE3) as default hashing algorithm for caching.
- [#1129](https://github.com/wasmerio/wasmer/pull/1129) Standard exception types for singlepass backend.

## 0.13.1 - 2020-01-16
- Fix bug in wapm related to the `package.wasmer_extra_flags` entry in the manifest

## 0.13.0 - 2020-01-15

Special thanks to [@repi](https://github.com/repi) and [@srenatus](https://github.com/srenatus) for their contributions!

- [#1153](https://github.com/wasmerio/wasmer/pull/1153) Added Wasmex, an Elixir language integration, to the README
- [#1133](https://github.com/wasmerio/wasmer/pull/1133) New `wasmer_trap` function in the C API, to properly error from within a host function
- [#1147](https://github.com/wasmerio/wasmer/pull/1147) Remove `log` and `trace` macros from `wasmer-runtime-core`, remove `debug` and `trace` features from `wasmer-*` crates, use the `log` crate for logging and use `fern` in the Wasmer CLI binary to output log messages.  Colorized output will be enabled automatically if printing to a terminal, to force colorization on or off, set the `WASMER_COLOR` environment variable to `true` or `false`.
- [#1128](https://github.com/wasmerio/wasmer/pull/1128) Fix a crash when a host function is missing and the `allow_missing_functions` flag is enabled
- [#1099](https://github.com/wasmerio/wasmer/pull/1099) Remove `backend::Backend` from `wasmer_runtime_core`
- [#1097](https://github.com/wasmerio/wasmer/pull/1097) Move inline breakpoint outside of runtime backend
- [#1095](https://github.com/wasmerio/wasmer/pull/1095) Update to cranelift 0.52.
- [#1092](https://github.com/wasmerio/wasmer/pull/1092) Add `get_utf8_string_with_nul` to `WasmPtr` to read nul-terminated strings from memory.
- [#1071](https://github.com/wasmerio/wasmer/pull/1071) Add support for non-trapping float-to-int conversions, enabled by default.

## 0.12.0 - 2019-12-18

Special thanks to [@ethanfrey](https://github.com/ethanfrey), [@AdamSLevy](https://github.com/AdamSLevy), [@Jasper-Bekkers](https://github.com/Jasper-Bekkers), [@srenatus](https://github.com/srenatus) for their contributions!

- [#1078](https://github.com/wasmerio/wasmer/pull/1078) Increase the maximum number of parameters `Func` can take
- [#1062](https://github.com/wasmerio/wasmer/pull/1062) Expose some opt-in Emscripten functions to the C API
- [#1032](https://github.com/wasmerio/wasmer/pull/1032) Change the signature of the Emscripten `abort` function to work with Emscripten 1.38.30
- [#1060](https://github.com/wasmerio/wasmer/pull/1060) Test the capi with all the backends
- [#1069](https://github.com/wasmerio/wasmer/pull/1069) Add function `get_memory_and_data` to `Ctx` to help prevent undefined behavior and mutable aliasing. It allows accessing memory while borrowing data mutably for the `Ctx` lifetime. This new function is now being used in `wasmer-wasi`.
- [#1058](https://github.com/wasmerio/wasmer/pull/1058) Fix minor panic issue when `wasmer::compile_with` called with llvm backend.
- [#858](https://github.com/wasmerio/wasmer/pull/858) Minor panic fix when wasmer binary with `loader` option run a module without exported `_start` function.
- [#1056](https://github.com/wasmerio/wasmer/pull/1056) Improved `--invoke` args parsing (supporting `i32`, `i64`, `f32` and `f32`) in Wasmer CLI
- [#1054](https://github.com/wasmerio/wasmer/pull/1054) Improve `--invoke` output in Wasmer CLI
- [#1053](https://github.com/wasmerio/wasmer/pull/1053) For RuntimeError and breakpoints, use Box<Any + Send> instead of Box<Any>.
- [#1052](https://github.com/wasmerio/wasmer/pull/1052) Fix minor panic and improve Error handling in singlepass backend.
- [#1050](https://github.com/wasmerio/wasmer/pull/1050) Attach C & C++ headers to releases.
- [#1033](https://github.com/wasmerio/wasmer/pull/1033) Set cranelift backend as default compiler backend again, require at least one backend to be enabled for Wasmer CLI
- [#1044](https://github.com/wasmerio/wasmer/pull/1044) Enable AArch64 support in the LLVM backend.
- [#1030](https://github.com/wasmerio/wasmer/pull/1030) Ability to generate `ImportObject` for a specific version WASI version with the C API.
- [#1028](https://github.com/wasmerio/wasmer/pull/1028) Introduce strict/non-strict modes for `get_wasi_version`
- [#1029](https://github.com/wasmerio/wasmer/pull/1029) Add the “floating” `WasiVersion::Latest` version.
- [#1006](https://github.com/wasmerio/wasmer/pull/1006) Fix minor panic issue when `wasmer::compile_with` called with llvm backend
- [#1009](https://github.com/wasmerio/wasmer/pull/1009) Enable LLVM verifier for all tests, add new llvm-backend-tests crate.
- [#1022](https://github.com/wasmerio/wasmer/pull/1022) Add caching support for Singlepass backend.
- [#1004](https://github.com/wasmerio/wasmer/pull/1004) Add the Auto backend to enable to adapt backend usage depending on wasm file executed.
- [#1068](https://github.com/wasmerio/wasmer/pull/1068) Various cleanups for the singlepass backend on AArch64.

## 0.11.0 - 2019-11-22

- [#713](https://github.com/wasmerio/wasmer/pull/713) Add AArch64 support for singlepass.
- [#995](https://github.com/wasmerio/wasmer/pull/995) Detect when a global is read without being initialized (emit a proper error instead of panicking)
- [#996](https://github.com/wasmerio/wasmer/pull/997) Refactored spectests, emtests and wasitests to use default compiler logic
- [#992](https://github.com/wasmerio/wasmer/pull/992) Updates WAPM version to 0.4.1, fix arguments issue introduced in #990
- [#990](https://github.com/wasmerio/wasmer/pull/990) Default wasmer CLI to `run`.  Wasmer will now attempt to parse unrecognized command line options as if they were applied to the run command: `wasmer mywasm.wasm --dir=.` now works!
- [#987](https://github.com/wasmerio/wasmer/pull/987) Fix `runtime-c-api` header files when compiled by gnuc.
- [#957](https://github.com/wasmerio/wasmer/pull/957) Change the meaning of `wasmer_wasix::is_wasi_module` to detect any type of WASI module, add support for new wasi snapshot_preview1
- [#934](https://github.com/wasmerio/wasmer/pull/934) Simplify float expressions in the LLVM backend.

## 0.10.2 - 2019-11-18

- [#968](https://github.com/wasmerio/wasmer/pull/968) Added `--invoke` option to the command
- [#964](https://github.com/wasmerio/wasmer/pull/964) Enable cross-compilation for specific target
- [#971](https://github.com/wasmerio/wasmer/pull/971) In LLVM backend, use unaligned loads and stores for non-atomic accesses to wasmer memory.
- [#960](https://github.com/wasmerio/wasmer/pull/960) Fix `runtime-c-api` header files when compiled by clang.
- [#925](https://github.com/wasmerio/wasmer/pull/925) Host functions can be closures with a captured environment.
- [#917](https://github.com/wasmerio/wasmer/pull/917) Host functions (aka imported functions) may not have `&mut vm::Ctx` as first argument, i.e. the presence of the `&mut vm::Ctx` argument is optional.
- [#915](https://github.com/wasmerio/wasmer/pull/915) All backends share the same definition of `Trampoline` (defined in `wasmer-runtime-core`).

## 0.10.1 - 2019-11-11

- [#952](https://github.com/wasmerio/wasmer/pull/952) Use C preprocessor to properly hide trampoline functions on Windows and non-x86_64 targets.

## 0.10.0 - 2019-11-11

Special thanks to [@newpavlov](https://github.com/newpavlov) and [@Maxgy](https://github.com/Maxgy) for their contributions!

- [#942](https://github.com/wasmerio/wasmer/pull/942) Deny missing docs in runtime core and add missing docs
- [#939](https://github.com/wasmerio/wasmer/pull/939) Fix bug causing attempts to append to files with WASI to delete the contents of the file
- [#940](https://github.com/wasmerio/wasmer/pull/940) Update supported Rust version to 1.38+
- [#923](https://github.com/wasmerio/wasmer/pull/923) Fix memory leak in the C API caused by an incorrect cast in `wasmer_trampoline_buffer_destroy`
- [#921](https://github.com/wasmerio/wasmer/pull/921) In LLVM backend, annotate all memory accesses with TBAA metadata.
- [#883](https://github.com/wasmerio/wasmer/pull/883) Allow floating point operations to have arbitrary inputs, even including SNaNs.
- [#856](https://github.com/wasmerio/wasmer/pull/856) Expose methods in the runtime C API to get a WASI import object

## 0.9.0 - 2019-10-23

Special thanks to @alocquet for their contributions!

- [#898](https://github.com/wasmerio/wasmer/pull/898) State tracking is now disabled by default in the LLVM backend. It can be enabled with `--track-state`.
- [#861](https://github.com/wasmerio/wasmer/pull/861) Add descriptions to `unimplemented!` macro in various places
- [#897](https://github.com/wasmerio/wasmer/pull/897) Removes special casing of stdin, stdout, and stderr in WASI.  Closing these files now works.  Removes `stdin`, `stdout`, and `stderr` from `WasiFS`, replaced by the methods `stdout`, `stdout_mut`, and so on.
- [#863](https://github.com/wasmerio/wasmer/pull/863) Fix min and max for cases involving NaN and negative zero when using the LLVM backend.

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
