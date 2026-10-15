# WASIX wasm tests

This directory contains integration tests that build small WASM programs and run them directly through `wasmer-wasix` (WASIX v1) and, for C fixtures, `wasixv2` (WASIX v2) from Rust. Most fixtures are C or C++ programs compiled with `wasixcc` / `wasix++`. The suite is the `wasm_tests` test target of the `wasix-tests` crate.

A working installation of `wasixcc` is required for this suite to work. The harness expects `wasixcc` and `wasix++` to be available on `PATH`.

## How the suite is structured

The `wasm_tests` test target discovers tests automatically from this
directory. Any subdirectory that contains one of these primary files is treated
as a test fixture:

- `build.sh` or other `*.sh` primary sources
- `Cargo.toml`
- `main.c`
- `main.cpp`
- `*.rs`

Discovery precedence within a fixture directory is:

1. `*.sh` shell primary sources
2. `Cargo.toml` (full Cargo project)
3. `main.c` / `main.cpp`
4. `*.rs` (each Rust source is an independent test)

The harness builds each discovered fixture, runs the resulting `main` module through the WASIX runner,
and registers one test per configuration each of the enabled engines.

Primary source files can contain inline directives that define the runnable configurations,
arguments, runtime environment, stdin, expected output, expected exit status, mapped directories, prefilled files, and file checks.
The supported directives are documented in [`mod.rs`](./mod.rs).

If a fixture has more than one `.sh` file, each shell file is treated as a
primary source, where `build.sh` is the default shell source name.

Fixtures with an explicit `Cargo.toml` are built as full Cargo projects. Directives
can be placed in `##Directive: Args` comments in the manifest.

The former `tests/wasi-fyi` shell suite now lives in [`wasi_fyi/`](./wasi_fyi/)
as Rust primary sources with inline directives.

## How building works

Source fixtures are copied into a per-test build directory under `wasm_tests/build/`.
The harness then builds them as follows:

- `main.c` is compiled with `CC`, or `wasixcc` if `CC` is unset.
- `main.cpp` is compiled with `CXX`, or `wasix++` if `CXX` is unset.
- `*.rs` is built with `cargo wasix build` using an ephemeral `Cargo.toml` generated
  in the build directory.
- `Cargo.toml` fixtures are built with `cargo wasix build` and the single binary
  artifact is copied to `main`.
- `build.sh` and other shell primary sources are executed with `bash`; the
  harness sets `CC=wasixcc`, `CXX=wasix++`, and
  `WASIXCC_DISCARD_UNSUPPORTED_FLAGS=yes`.

Every build must produce an executable WebAssembly output named `main`.

If a fixture needs compiler environment overrides, add `BuildEnv` directives to
the primary source, for example `//#BuildEnv: WASIXCC_PIC=1` in C/C++ sources or
`##BuildEnv: WASIXCC_PIC=1` in shell sources.

## Running the tests

Run the suite with `cargo nextest run -p wasix-tests --release` (or
`cargo test`); standard Cargo and nextest filtering both work. Before running the suite, make sure
`wasixcc` is installed and available in your shell environment.
Rust fixtures also require `cargo-wasix` on `PATH` (`cargo install cargo-wasix`).

Single-file Rust fixtures build with two Rust toolchains by default (see the
`Toolchains` directive): `wasix` (cargo-wasix), which runs on every engine
except Singlepass — the WASIX toolchain emits exception-handling opcodes
Singlepass does not support — and `wasip1` (`rustc --target wasm32-wasip1`,
nightly when the source uses `#![feature(...)]`), which runs on every engine.
The wasip1 variants need `rustup target add wasm32-wasip1` (plus the same
target on nightly).

We do not publish the WASIX Rust toolchain for every platform yet, so the
`wasix` variants are only collected on hosts that have one: gnu Linux on x86_64
and aarch64, and macOS on aarch64. They are skipped on musl, which has no
toolchain, and on Windows, which `wasixcc` does not cover either. The `wasip1`
variants build everywhere, so single-file Rust fixtures keep running on those
hosts.

On macOS, this suite collects and runs the LLVM variants only because Cranelift
exception-handling support is still incomplete there:

## WASIX v2

Every configuration of a C fixture (`main.c`, or a shell primary source that
uses only `CC`) also gets a WASIX v2 variant, named like its v1 counterpart
with a `v2` component before the engine:
`wasm/basic/helloworld/default/v2/cranelift`. v1 test names are unchanged.
C++ and Rust fixtures have no v2 variant: there is no libc++ or Rust target
for v2 yet.

The v2 variant builds the fixture with `wasixcc` in v2 mode
(`WASIXCC_TARGET=wasm32-wasixv2`) and runs it with
`wasixv2::Machine::run_process` on its default engine (Cranelift), with the
fixture's arguments, environment and stdin, capturing stdout and stderr. It is
registered only when the v2 toolchain is found; otherwise the suite prints one
`skipping WASIX v2 tests: <why>` line. It looks for:

- `wasixcc` with v2 support: `$WASIXCC`, else `wasixcc` on `PATH`.
- the v2 LLVM: `$WASIX_TESTS_V2_LLVM_LOCATION`, else `~/.wasixcc/llvm-wasixv2`.
- the v2 sysroot: `$WASIX_TESTS_V2_SYSROOT`, else `~/.wasixcc/sysroot-wasixv2`.

The locations have their own variables because the `WASIXCC_*` ones would
also reach the v1 builds. A v2 build sets `WASIXCC_LLVM_LOCATION` and
`WASIXCC_SYSROOT` from them. A v2 guest that runs longer than
`$WASIX_TESTS_V2_TIMEOUT_SECS` (default 60) fails the test.

The v2 runner does not honour everything yet. These directives mark a v2
variant as ignored with `unsupported on v2: <directive>`: `MappedDirectory`,
`CurrentDirectory`, `PrefilledFile`, `ExpectedFile`, `FileSystems`, and
`BuildEnv` values that `wasixcc` rejects for v2 (`WASIXCC_PIC=1`,
`WASIXCC_WASM_EXCEPTIONS=no|legacy`). The harness's default mounts of the
build directory (as the cwd, `/lib` and `/data`) do not exist on v2: the guest
sees the machine's in-memory root with an empty `/tmp`, and `/` as its cwd.
`SkipEngine: cranelift` applies to v2; `MinimalLibc` does not, since it
concerns the v1 sysroot versions.

[`v2-expected-failures.txt`](../../v2-expected-failures.txt) lists the v2
variants known to fail; its header describes the format. A listed test that
fails passes the suite, and one that passes fails it with "unexpectedly
passed", so the list has to shrink as v2 gains features. A v2 failure reports
which syscalls the kernel answered with `ENOSYS` and the last syscalls the
guest made.
