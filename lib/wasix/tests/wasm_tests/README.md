# WASIX wasm tests

This is a collection of tests that run wasm modules using WASIX. Most of the tests use `wasixcc` to compile C code with `wasix-libc` to generate the modules.

The general idea of this test suite is to run the modules directly from rust using the wasmer/wasix crates. This way it is easy to debug them with a debugger.

## Adding tests

The tests are organized into somewhat related sets. For each set there is a `<setname>.rs` module which defines the tests. For each tests there is a test directory at `<setname>/<testname>/` which contains the sources for the test (`build.sh`, C files, precompiled wasm files, etc...).

Tests should always run the wasm modules with the `run_wasm() -> ()` or `run_wasm_with_result() -> output + exitcode` helper functions/

Look at [`basic_tests`](./basic_tests/) for an example for a normal test set.

When adding a new testset module remember importing the module in [`wasm_tests/mod.rs`](./mod.rs).

### Normal tests

For generating modules there is a `run_build_script()` helper function that calls `build.sh` in the respective tests directory. `build.sh` should generate a `main` wasm module next to the build script. Using this structure a normal test case looks like:

```rust
#[test]
fn test_helloworld() {
    // Run build.sh in the test directory
    let wasm_path = run_build_script(file!(), "helloworld").unwrap();
    let test_dir = wasm_path.parent().unwrap();
    // Run the generated wasm module
    run_wasm(&wasm_path, test_dir).unwrap();
}
```

`build.sh` should be written in a way that they can also produce native executables. In most cases this means not to call `wasixcc` directly but instead use `$CC` and `$CXX` and to check that it still builds with them set to `clang`/`clang++`. There is no automated testing for this.

### Tests with precompiled wasm files

You should avoid creating tests with precompiled wasm files. If possible stay close to the normal test architecture with `build.sh` and C files. If that is not possible you can also commit precompiled wasm files, but please create a README.md in the test directory in that case.

### Tests that are only expected to work on WASIX

Ideally the C based tests compile and run for both native and WASIX. For now we don't mark tests that only support WASIX (for example the ones using the context switching, dynamic calling, or closure APIs).

In the future we would ideally have some way to mark them and some infra to test whether they work on native clang/gcc and maybe even emscripten.