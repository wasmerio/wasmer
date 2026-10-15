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

## Testing WASIX v2

WASIX v2 (`lib/wasixv2`, design in `lib/wasixv2/DESIGN.md`) spans three
repositories besides this one, all wired in as submodules. Run
`git submodule update --init` first: a missing submodule stops the cargo
workspace from loading.

| Repository | Here | Holds |
| ---------- | ---- | ----- |
| [wasixv2-abi](https://github.com/wasix-org/wasixv2-abi) | `lib/wasixv2/codegen` | The ABI (annotated WIT) and `wasixv2-codegen` |
| [wasixv2-tests](https://github.com/wasix-org/wasixv2-tests) | `tests/wasixv2` | The test suite and its runner |
| [libc2](https://github.com/wasix-org/libc2) | (not a submodule) | The musl-based libc; mounts both repos above under `wasixv2/` |

### Crate tests

```bash
cargo test -p wasixv2 -p wasixv2-abi -p wasixv2-kernel -p wasixv2-codegen  # and the other lib/wasixv2 crates
```

These are plain Rust: the kernel's unit tests, the facade's WAT tests, and
`lib/wasixv2/abi/tests/generated.rs`, which checks that the generated
crates match the ABI at the pinned submodule commit. After an ABI or
generator change, regenerate with
`WASIXV2_CODEGEN_BLESS=1 cargo test -p wasixv2-abi --test generated`.

### The test suite

```bash
make test-wasixv2   # builds the wasmer CLI, then runs tests/wasixv2
```

The suite builds every program under `tests/wasixv2/suite/` with `wasixcc`
for `wasm32-wasixv2` and runs it with that `wasmer`. It needs, from the
environment:

- `wasixcc` with wasixv2 support on `PATH` (unreleased:
  wasix-org/wasixcc#87).
- The wasix-org LLVM fork (`wasix-22.1.8` branch), via
  `WASIXCC_LLVM_LOCATION`.
- A libc2 sysroot, via `WASIXCC_SYSROOT` (`<libc2>/build-wasixv2/sysroot-wasixv2`
  after `make wasixv2-sysroot` in libc2).

Filter tests with `WASIXV2_TESTS_ARGS=<filter>`. Most of the suite is
expected to fail until wasixv2 is complete: `tests/wasixv2/expected-failures.txt`
lists those tests, and a listed test that starts passing fails the run.
Regenerate the list with `WASIXV2_TESTS_BLESS=1`, then commit it in
wasixv2-tests. CI does not run the suite yet.

### Rules

- Do not add wasixv2 program tests to this repository, and never check in
  prebuilt wasm for them. New tests go to wasixv2-tests, built from source
  with `wasixcc`.
- WASIX v1 tests stay in `lib/wasix/tests`; never copy them to wasixv2-tests.
- An ABI change lands in wasixv2-abi first. Then bump the submodule in both
  wasmer and libc2 to the same commit, regenerating in the same commit as
  the bump.

## Personal execution preferences

The gitignored `AGENTS.override.md` is the user's personal addendum. When
it exists, it overrides this file's defaults.
