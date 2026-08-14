# Testing

Thanks to the [WebAssembly spec tests](https://github.com/WebAssembly/testsuite)
we can ensure 100% compatibility with the WebAssembly spec test suite.

All make targets run with `--locked`. If `Cargo.lock` drifted, every target
fails. Commit lockfile changes only when intended.

| Goal                          | Command                                                                      |
| ----------------------------- | ---------------------------------------------------------------------------- |
| Full local suite              | `make test` (needs `cargo-nextest`; `make require-nextest` installs it)      |
| WAST spec tests, all backends | `make test-wast`                                                             |
| WAST for one backend          | `make test-singlepass`, `make test-cranelift`, `make test-llvm`              |
| One test by name filter       | `cargo test --release --tests --features cranelift -- cranelift::spec::simd` |
| One crate                     | `cargo test -p wasmer-wasix --features sys`                                  |
| CLI tests                     | `make test-wasmer-cli`                                                       |
| Examples                      | `make test-examples`                                                         |
| C API                         | `make test-capi`                                                             |

`make test` automatically detects the compilers available on your system.
Follow the [Building from Source](./BUILD.md) guide to prepare your system
with the requirements for each backend.

> [!CAUTION]
> `make test-wast` uses `cargo test`, not nextest, because the tests must
> share one process. Do not convert it to nextest.

## Skipping tests

If a test fails only on one backend or platform, add a line to
`tests/ignores.txt` with a reason (`<compiler> <test> # reason`). Do not
chase Singlepass failures for SIMD, relaxed SIMD, exception handling, or
wide arithmetic. Singlepass does not support these proposals.

To debug a failing WAST test, see [DEBUGGING.md](./DEBUGGING.md).

## WASIX guest-side tests

WASIX guest-side tests live in `lib/wasix/tests/wasm_tests/`. They need
`wasixcc` and `cargo-wasix` on PATH. Fixture directives are documented in
`lib/wasix/tests/wasm_tests.rs`.

## CI coverage gaps

macOS and musl CI jobs run on a PR only when the PR carries the `macos` or
`musl` label (see [CONTRIBUTING.md](./CONTRIBUTING.md)). A PR can pass CI
and still break `main` on those platforms.
