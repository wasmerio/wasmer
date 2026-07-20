# WASIX wasm tests

This directory contains integration tests that build small WASM programs and run them directly through `wasmer-wasix` from Rust. Most fixtures are C or C++ programs compiled with `wasixcc` / `wasix++`.

A working installation of `wasixcc` is required for this suite to work. The harness expects `wasixcc` and `wasix++` to be available on `PATH`.

## How the suite is structured

The `wasix-wasm` integration test target discovers tests automatically from this
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
The supported directives are documented in [`../wasm_tests.rs`](../wasm_tests.rs).

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

These tests run through the normal `wasix` integration test target, so standard
Cargo and nextest filtering both work. Before running the suite, make sure
`wasixcc` is installed and available in your shell environment.
Rust fixtures also require `cargo-wasix` on `PATH` (`cargo install cargo-wasix`).

Single-file Rust fixtures build with two toolchains by default (see the
`Toolchains` directive): `wasix` (cargo-wasix), which runs on every engine
except Singlepass, and `wasip1` (`rustc --target wasm32-wasip1`, nightly when
the source uses `#![feature(...)]`), which runs on Singlepass only — the WASIX
toolchain emits exception-handling opcodes Singlepass does not support. The
wasip1 variants need `rustup target add wasm32-wasip1` (plus the same target on
nightly).

The WASIX Rust toolchain is not published for every platform, so the
wasix-toolchain fixtures can alternatively be prebuilt on a supported host and
reused:

- `WASM_TESTS_BUILD_ONLY_DIR=<dir>` builds the Rust fixtures into `<dir>`
  without running any tests (CI runs this through
  `make build-wasm-tests-fixtures` on linux-x64). The artifacts are
  engine-independent and are built through the Cranelift trials, so this must
  run on a host that collects them (i.e. not macOS).
- `WASM_TESTS_PREBUILT_DIR=<dir>` makes the suite consume those prebuilt
  artifacts instead of invoking `cargo wasix build`, removing the need for
  `cargo-wasix` and the WASIX Rust toolchain on the test host.

On macOS, this suite collects and runs the LLVM variants only because Cranelift
exception-handling support is still incomplete there:
