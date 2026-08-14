# Debugging the runtime

## Logging

Set `RUST_LOG` with standard EnvFilter syntax, for example:

```bash
RUST_LOG="warn,wasmer_wasix=trace" wasmer run file.wasm
```

The `-v` to `-vvvv` flags map to warn/info/debug/trace for the targets
`wasmer`, `wasmer_wasix`, and `virtual_fs`. Implementation:
`lib/cli/src/logging.rs`.

`make build-wasmer-debug` builds a debug binary with tokio-console support.

## Run the workspace CLI directly

```bash
cargo run -p wasmer-cli --features cranelift -- run file.wasm
```

## Inspect and author Wasm

- Inspect a module with `wasm-tools print file.wasm`. Pipe through `head`
  or `grep` — the output is large.
- Author test cases with `wasm-tools parse file.wat -o file.wasm`.
- Reduce hard cases with `wasm-opt` and `creduce`.

## Compile a C repro to WASIX

```bash
WASIXCC_WASM_EXCEPTIONS=1 WASIXCC_PIC=1 wasixcc -g -O0 file.c -o file.wasm
```

## Debug a failing WAST test

Find the `.wast` file under `tests/wast/`, then rerun with the
backend-filtered command from [TEST.md](./TEST.md). The runner prints
`Running wast <path>`.
