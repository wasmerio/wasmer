# WASIX wasm tests

This is a collection of tests that run wasm modules using WASIX. Most of the tests use `wasixcc` to compile C code with `wasix-libc` to generate the modules.

The general idea of this test suite is to run the modules directly from rust using the wasmer/wasix crates. This way it is easy to debug them with a debugger.

## Adding tests

The tests are organized into somewhat related sets. For each set there is a `<setname>.rs` module which defines the tests. For each test there is a test directory at `<setname>/<testname>/` which contains the sources for the test (C/C++ files, optionally a `build.sh`, precompiled wasm files, etc.).

Look at [`basic_tests`](./basic_tests/) for an example of a normal test set.

When adding a new testset module remember importing the module in [`wasm_tests/mod.rs`](./mod.rs).

### Normal tests

Use the `wasm_test!` macro — it generates the `#[test]` function and handles building and running automatically:

```rust
// Assert exit 0
wasm_test!(test_helloworld, "helloworld");

// Assert non-zero exit
wasm_test!(test_exits_nonzero, "exit-nonzero", should_fail);

// Assert trimmed stdout matches a string
wasm_test!(test_prints_hello, "hello", stdout = "hello world");

// With extra attributes (cfg, ignore, etc.)
wasm_test!(#[cfg(unix)] test_context, "ctx");
```

The macro calls `run_build_script` to compile the test and then runs it.

### Building tests

`run_build_script` looks for a way to build the test in this order:

1. **`build.sh`** — if present, it is executed with `bash`. `$CC` and `$CXX` are set to `wasixcc`/`wasix++`. Write `build.sh` using `$CC`/`$CXX` so the same script works with native compilers too.
2. **Auto-build** — if there is no `build.sh`, the script looks for a single `.c` or `.cpp` file in the test directory and compiles it directly with `wasixcc`/`wasix++`. This covers the common case of a single-source test with no special build logic.

Only add a `build.sh` when the test needs something the auto-build cannot handle: multiple source files, shared libraries, special linker flags, etc.

### Per-test compiler flags (`build.env`)

If a test needs extra environment variables for the compiler (e.g. `WASIXCC_WASM_EXCEPTIONS=1`), create a `build.env` file in the test directory with one `KEY=VALUE` entry per line. These variables are injected into both `build.sh` and auto-build invocations.

### Tests with precompiled wasm files

You should avoid creating tests with precompiled wasm files. If possible stay close to the normal test architecture with `build.sh` and C files. If that is not possible you can also commit precompiled wasm files, but please create a README.md in the test directory in that case.

### Tests that are only expected to work on WASIX

Ideally the C based tests compile and run for both native and WASIX. For now we don't mark tests that only support WASIX (for example the ones using the context switching, dynamic calling, or closure APIs).

In the future we would ideally have some way to mark them and some infra to test whether they work on native clang/gcc and maybe even emscripten.
