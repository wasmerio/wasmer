# Working in this repo as an agent

Before your first edit, read `docs/CONTRIBUTING.md` and `docs/BUILD.md` in
full, plus the row below covering whatever you are about to touch. Skipping one
of those docs does not exempt you from what it says.

Always read:

| Read                   | For                                                    |
| ---------------------- | ------------------------------------------------------ |
| `docs/CONTRIBUTING.md` | PR expectations, commit format, lint gates, code style |
| `docs/ARCHITECTURE.md` | the crate map, one place per concern, submodules       |
| `docs/BUILD.md`        | toolchain setup, compiler backends, fast iteration     |
| `docs/TEST.md`         | test suites, WAST spec tests, `tests/ignores.txt`      |
| `docs/SECURITY.md`     | supported versions, how to report vulnerabilities      |

Read if necessary:

| If necessary        | For                            |
| ------------------- | ------------------------------ |
| `docs/journal.md`   | snapshot and restore internals |
| `docs/PACKAGING.md` | distro packaging constraints   |
| `docs/RISCV.md`     | state of RISC-V support        |

Trust the repository more than any doc. `Makefile` targets and `--help`
output beat external docs, and docs.wasmer.io lags this repository.

## Before you write code

Every new test, fixture, syscall implementation, or backend arm has a
sibling in this repo that already does the same kind of thing. Find it and
match its location, naming, and structure. If you cannot find one, say so
before inventing a layout.

Before implementing a new mechanism, state the design in one or two
sentences and get agreement. Prioritise the smallest diff that slots into
existing machinery. Work already done is not an argument for keeping a
shape: if the design is wrong, say so rather than defending it.

## Attribution

An agent MUST disclose that it is the one that committed its changes, and
MUST NOT feign being a user. The human user is fully responsible for all
contributions.

## Debugging the Runtime

### Logging

Set `RUST_LOG` with standard EnvFilter syntax, for example:

```bash
RUST_LOG="warn,wasmer_wasix=trace" wasmer run file.wasm
```

The `-v` to `-vvvv` flags map to warn/info/debug/trace for the targets
`wasmer`, `wasmer_wasix`, and `virtual_fs`.

`make build-wasmer-debug` builds a debug binary with tokio-console support.

### Run the Workspace CLI Directly

```bash
cargo run -p wasmer-cli --features cranelift -- run file.wasm
```

### Inspect and Author Wasm

- Inspect a module with `wasm-tools print file.wasm`. Pipe through `head`
  or `grep` — the output is large.
- Author test cases with `wasm-tools parse file.wat -o file.wasm`.
- Reduce hard cases with `wasm-opt` and `creduce`.

### Compile a C Repro to WASIX

```bash
WASIXCC_WASM_EXCEPTIONS=1 WASIXCC_PIC=1 wasixcc -g -O0 file.c -o file.wasm
```

### Debug a Failing WAST Test

Find the `.wast` file under `tests/wast/`, then rerun with the
backend-filtered command from [docs/TEST.md](./docs/TEST.md). The runner
prints `Running wast <path>`.

## Personal execution preferences

The gitignored `AGENTS.override.md` is the user's personal addendum. When
it exists, it overrides this file's defaults.
