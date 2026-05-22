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
arguments, expected output, expected exit status, mapped directories, prefilled files, and file checks.
The supported directives are documented in [`../wasm_tests.rs`](../wasm_tests.rs).

If a fixture has more than one `.sh` file, each shell file is treated as a
primary source, where`build.sh` is the default shell source name.

## How building works

Source fixtures are copied into a per-test build directory under `wasm_tests/build/`.
The harness then builds them as follows:

- `main.c` is compiled with `CC`, or `wasixcc` if `CC` is unset.
- `main.cpp` is compiled with `CXX`, or `wasix++` if `CXX` is unset.
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

On macOS, this suite is opt-in because Cranelift exception-handling support is
still incomplete there. Set `WASMER_ENABLE_MACOS_WASM_TESTS=1` to collect and
run the macOS-supported LLVM variants:

```sh
WASMER_ENABLE_MACOS_WASM_TESTS=1 cargo test --test wasm_tests wasm/context_switching/contexts_with_signals
```
