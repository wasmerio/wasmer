# WASIX wasm tests

This directory contains integration tests that build small WASM programs and run them directly through `wasmer-wasix` from Rust. Most fixtures are C or C++ programs compiled with `wasixcc` / `wasix++`.

A working installation of `wasixcc` is required for this suite to work. The harness expects `wasixcc` and `wasix++` to be available on `PATH`.

## How the suite is structured

The `wasix-wasm` integration test target discovers tests automatically from this
directory. Any subdirectory that contains one of these primary files is treated
as a test fixture:

- `main.c`
- `main.cpp`
- `$name.sh`

The harness builds each discovered fixture, runs the resulting `main` module through the WASIX runner,
and registers one test per configuration each of the enabled engines.

Primary source files can contain inline directives that define the runnable configurations,
arguments, runtime environment, stdin, expected output, expected exit status, mapped directories, prefilled files, and file checks.
The supported directives are documented in [`../wasm_tests.rs`](../wasm_tests.rs).

If a fixture has more than one `.sh` file, each shell file is treated as a
primary source, where `build.sh` is the default shell source name.

The former `tests/wasi-fyi` shell suite now lives in [`wasi_fyi/`](./wasi_fyi/)
as Rust primary sources with inline directives.

## How building works

Source fixtures are copied into a per-test build directory under `wasm_tests/build/`.
The harness then builds them as follows:

- `main.c` is compiled with `CC`, or `wasixcc` if `CC` is unset.
- `main.cpp` is compiled with `CXX`, or `wasix++` if `CXX` is unset.
- `*.rs` is compiled with `rustc --target=wasm32-wasip1`.
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
Rust `.rs` tests also require `rustc` with the `wasm32-wasip1` target installed.

On macOS, this suite collects and runs the LLVM variants only because Cranelift
exception-handling support is still incomplete there:
