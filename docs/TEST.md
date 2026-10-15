# Testing

Thanks to the [WebAssembly spec tests](https://github.com/WebAssembly/testsuite)
we can ensure 100% compatibility with the WebAssembly spec test suite.

Run the full local suite with `make test` (needs `cargo-nextest`;
`make require-nextest` installs it). It automatically detects the
compilers available on your system. Follow the
[Building from Source](./BUILD.md) guide to prepare your system with the
requirements for each backend.

The [Makefile](../Makefile) lists the individual test targets and their
purpose: the WAST spec tests per backend, the CLI, the examples, the
C API, and more.

It is also possible to test specific parts. One crate:

```bash
cargo test -p wasmer-wasix --features sys
```

One test by name filter:

```bash
cargo test --release --tests --features cranelift -- cranelift::spec::simd
```

## Skipping Tests

If a test fails only on one backend or platform, add a line to
`tests/ignores.txt` with a reason (`<compiler> <test> # reason`). Do not
chase Singlepass failures for SIMD, relaxed SIMD, exception handling, or
wide arithmetic. Singlepass does not support these proposals.

To debug a failing WAST test, see
[AGENTS.md](../AGENTS.md#debugging-the-runtime).

## WASIX Guest-Side Tests

WASIX guest-side tests live in `lib/wasix/tests/wasm_tests/`. They need
`wasixcc` and `cargo-wasix` on PATH. Fixture directives are documented in
`lib/wasix/tests/wasm_tests.rs`.

## WASIX v2 Test Suite

The wasixv2 suite lives in its own repository,
[wasixv2-tests](https://github.com/wasix-org/wasixv2-tests), checked out at
`tests/wasixv2`; libc2 runs the same suite. `make test-wasixv2` builds the
`wasmer` CLI and runs it. It needs `wasixcc` with `wasm32-wasixv2` support on
PATH; `WASIXCC_LLVM_LOCATION` and `WASIXCC_SYSROOT` point it at a local LLVM
fork and libc2 sysroot. Most tests are expected to fail until wasixv2 is
complete: the suite's `expected-failures.txt` lists them. CI does not run it
yet.

## CI Coverage Gaps

macOS and musl CI jobs run on a PR only when the PR carries the `macos` or
`musl` label (see [CONTRIBUTING.md](./CONTRIBUTING.md)). A PR can pass CI
and still break `main` on those platforms.
