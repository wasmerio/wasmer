# WASIX wasm tests

This directory contains integration tests that build small WASM programs and run them directly through `wasmer-wasix` from Rust. Most fixtures are C or C++ programs compiled with `wasixcc` / `wasix++`.

A working installation of `wasixcc` is required for this suite to work. The harness expects `wasixcc` and `wasix++` to be available on `PATH`.

## How the suite is structured

There are two layers:

1. Rust test modules in this directory such as `basic_tests.rs`, `process_tests.rs`, or `threadlocal_tests.rs`.
2. Fixture directories that sit next to the module name and contain the source code and optional build files.

The mapping is driven by `run_build_script(file!(), ...)` in [`mod.rs`](./mod.rs):

- `basic_tests.rs` loads fixtures from `basic_tests/...`
- `process_tests.rs` loads fixtures from `process_tests/...`
- `fd_fdflags_get.rs` can use its own directory directly

Examples:

- [`basic_tests.rs`](./basic_tests.rs) defines `wasm_test!(test_helloworld, "helloworld");`
- that resolves to [`basic_tests/helloworld/`](./basic_tests/helloworld/)
- [`fd_fdflags_get.rs`](./fd_fdflags_get.rs) defines `wasm_test!(fd_fdflags_get, "");`
- that resolves to [`fd_fdflags_get/`](./fd_fdflags_get/)

Most tests use the `wasm_test!` macro from [`mod.rs`](./mod.rs). It:

- builds the fixture with `run_build_script`
- runs the resulting module with the WASIX runner
- asserts success, failure, exit code, or stdout depending on the macro form

Some tests are written manually instead of using the macro when they need custom runner setup, such as mapped directories, a custom working directory, or multiple invocations of the same compiled fixture. [`process_tests.rs`](./process_tests.rs) is the main example of that pattern.

## How building works

For a fixture directory, `run_build_script` builds in this order:

1. `build.sh`, if present
2. automatic single-file compilation if there is no `build.sh`

### `build.sh`

If a fixture has a `build.sh`, the test harness executes it with `bash` in the fixture directory and sets:

- `CC=wasixcc`
- `CXX=wasix++`
- `WASIXCC_DISCARD_UNSUPPORTED_FLAGS=yes`

Use `$CC` and `$CXX` inside the script instead of hardcoding compiler names.

Add `build.sh` when the fixture needs:

- multiple source files
- shared or dynamic libraries
- special linker flags
- multiple output artifacts

### Auto-build

If there is no `build.sh`, the harness looks for a single compilable source file using this priority:

1. `main.c`
2. `main.cpp`
3. the only `.c` file in the directory
4. the only `.cpp` file in the directory

That source is compiled to an output named `main`.

### `build.env`

If a fixture needs compiler environment overrides, add a `build.env` file with one `KEY=VALUE` entry per line. The harness applies those variables to both `build.sh` and auto-build mode.

Examples already in the tree use this for settings such as `WASIXCC_PIC=1` or `WASIXCC_WASM_EXCEPTIONS=no`.

## Adding a test

### Common case: a normal fixture

1. Pick the Rust module that should own the test, or create a new one.
2. Add a fixture directory under the matching path.
3. Put the fixture sources there.
4. Add a `wasm_test!` entry to the Rust module.
5. If you created a new Rust module, import it from [`mod.rs`](./mod.rs).

Minimal example:

```text
wasm_tests/
├── basic_tests.rs
└── basic_tests/
    └── hello-new/
        └── main.c
```

```rust
wasm_test!(test_hello_new, "hello-new");
```

Useful `wasm_test!` forms:

```rust
wasm_test!(test_ok, "hello");
wasm_test!(test_fails, "exit-nonzero", should_fail);
wasm_test!(test_exit_code, "abort-case", exit_code = 134);
wasm_test!(test_stdout, "print-case", stdout = "hello world");
wasm_test!(test_with_args, "arg-case", args = ["case-name"]);
wasm_test!(#[ignore = "flaky on CI"] test_ignored, "fixture");
```

Use `""` or `"."` as the fixture path when the sources live directly in the directory that matches the Rust file stem.

### Custom Rust test

Drop to a manual `#[test]` when the fixture needs more than the macro supports. Typical reasons:

- mount host directories with `MappedDirectory`
- set a guest current directory
- run the same compiled module multiple times with different args
- assert on stdout or stderr in a custom way
- run a secondary output file instead of `main`

In that case, follow the pattern used in [`process_tests.rs`](./process_tests.rs):

```rust
#[test]
fn test_custom() {
    let wasm = run_build_script(file!(), "my-fixture").unwrap();
    let result = run_wasm_with_runner_config(&wasm, wasm.parent().unwrap(), |runner| {
        runner.with_args(["example"]);
    })
    .unwrap();

    ensure_wasm_run_succeeded(&result).unwrap();
}
```

## Running the tests

These tests live in the `wasmer-wasix` crate and are compiled as integration tests.

Before running them, make sure `wasixcc` is installed and working in your shell environment.

> [!WARNING]
> The auto-build path respects the `CC` / `CXX` environment variables. If your shell has `CC` set to a native compiler (e.g. `CC=gcc`), tests will silently compile to a native executable instead of WebAssembly, producing invalid test artifacts and confusing failures. Clear `CC` before running the suite.

```bash
unset CC CXX
# or unset for a single invocation without affecting the current shell:
env -u CC -u CXX cargo test ...
```

Run the main WASIX wasm suite:

```bash
cargo test -p wasmer-wasix --features sys --test wasix-wasm -- --nocapture
```

Run all `wasmer-wasix` tests, including this suite:

```bash
cargo test -p wasmer-wasix --features sys
```

Run a single test by name:

```bash
cargo test -p wasmer-wasix --features sys --test wasix-wasm test_helloworld -- --nocapture
```

Run a subset by filter:

```bash
cargo test -p wasmer-wasix --features sys --test wasix-wasm process_tests -- --nocapture
```

On macOS there is also a separate integration test target for socket-specific coverage:

```bash
cargo test -p wasmer-wasix --features sys --test socket_wasm -- --nocapture
```

## Notes and conventions

- Prefer source-based fixtures over committing prebuilt `.wasm` files.
- If a prebuilt artifact is unavoidable, document how it was produced in a fixture-local `README.md`.
- Keep fixtures as small as possible. Add `build.sh` only when auto-build is not enough.
- Reuse `build.env` for per-fixture compiler settings instead of exporting variables inside Rust tests when possible.
