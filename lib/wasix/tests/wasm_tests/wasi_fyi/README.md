# wasi-fyi fixtures

These fixtures were ported from the legacy `tests/wasi-fyi` shell test suite into the `wasmer-wasix` Rust integration harness.

Each `*.rs` file is a primary source discovered by the shared `wasm_tests`
harness. The source-level directives define per-test inputs and expectations:

- `Args` supplies command-line arguments.
- `Env` supplies runtime environment variables.
- `StdinFile` supplies stdin bytes from a fixture file.
- `ExpectedStdoutFile` and `ExpectedStderrFile` assert exact output.
- `ExpectedExitCode` asserts the expected process exit code.

Common runner setup lives in `wasi-fyi.config` and is included explicitly with
`AbstractConfigFile`.

The original fixture license is preserved in `LICENSE`.
