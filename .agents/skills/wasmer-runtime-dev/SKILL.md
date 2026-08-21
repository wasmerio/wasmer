---
name: wasmer-runtime-dev
description: Build, test, lint, and debug the Wasmer runtime in the wasmerio/wasmer repository, and prepare changes that pass CI. Use when you contribute to the runtime, the CLI, a compiler backend (Singlepass, Cranelift, LLVM, V8), WASIX internals, or the C API. Also use when a build fails with LLVM or feature errors, when you run or skip WAST spec tests, or when you prepare a commit or PR for this repository.
license: MIT
metadata:
  author: wasmerio
---

# Contribute to the Wasmer runtime

This skill is for changes to the wasmerio/wasmer repository itself. For use
of the `wasmer` CLI as a product, read the `wasmer-local` skill. For app
deployment, read the `wasmer-edge` skill.

Trust the repository more than this file. Trust `Makefile` targets and
`--help` output more than external docs. Code style: sparse "why" comments,
short single-responsibility functions.

## Repository map

| Path | Content |
|---|---|
| `lib/api` | Public `wasmer` crate. Flavors: `sys` (native, default), `v8`, `js`. |
| `lib/cli` | CLI crate `wasmer-cli`. Binaries: `wasmer`, `wasmer-headless`. |
| `lib/compiler` | Compiler framework and traits. |
| `lib/compiler-{singlepass,cranelift,llvm}` | The three compiler backends. |
| `lib/vm` | Low-level VM runtime. |
| `lib/wasix` | The WASIX implementation (syscalls, processes, threads). |
| `lib/journal` | Snapshot and restore support. See `docs/journal.md`. |
| `lib/virtual-fs`, `lib/virtual-net`, `lib/virtual-io` | Host abstraction layers. |
| `lib/c-api` | C API (`cdylib` named `wasmer`). |
| `lib/backend-api` | GraphQL client for the Wasmer registry. |
| `lib/config`, `lib/package` | `wasmer.toml` parsing and webc packaging. |
| `tests/compilers` | Integration tests (`wast.rs`, `traps.rs`, `issues.rs`, more). |
| `tests/wast/{spec,wasmer}` | WAST test files. `spec` is a submodule. |
| `tests/ignores.txt` | Per-compiler test skip list: `<compiler> <test> # reason`. |

The root crate is a meta-crate. The root `build.rs` generates one WAST spec
test per compiler from `tests/wast/spec`.

## Build

Prerequisites: Rust 1.95 (pinned in `rust-toolchain.toml`, edition 2024).
The LLVM backend needs LLVM 22 exactly.

1. Build the release CLI: `make build-wasmer`.
   The binary is `./target/release/wasmer`.
2. Read the `Enabled Compilers:` banner that each make target prints.
   The Makefile silently omits backends it cannot detect.
3. For fast iteration, run `make check`, or build one crate:

```bash
cargo build -p wasmer-cli --features cranelift
```

- If LLVM 22 is not installed, run `ENABLE_LLVM=0 make build-wasmer`.
  The error `Didn't find usable system-wide LLVM` means LLVM 22 is missing.
  If `llvm-config-22` is not on PATH, set `LLVM_SYS_221_PREFIX` manually.
- Backend toggles: `ENABLE_CRANELIFT`, `ENABLE_LLVM`, `ENABLE_SINGLEPASS`,
  `ENABLE_V8`, each `0` or `1`. V8 is never autodetected.
- `make build-wasmer-debug` builds a debug binary with tokio-console
  support. C API: `make build-capi`.

CAUTION: Do not build with `cargo build --workspace --features <backend>`.
Workspace-level features do not reach subcrates. The result is a headless
binary that cannot compile Wasm. Use `-p wasmer-cli` or the Makefile.

## Test

All make targets run with `--locked`. If `Cargo.lock` drifted, every target
fails. Commit lockfile changes only when intended.

| Goal | Command |
|---|---|
| Full local suite | `make test` (needs `cargo-nextest`; `make require-nextest` installs it) |
| WAST spec tests, all backends | `make test-wast` |
| WAST for one backend | `make test-singlepass`, `make test-cranelift`, `make test-llvm` |
| One test by name filter | `cargo test --release --tests --features cranelift -- cranelift::spec::simd` |
| One crate | `cargo test -p wasmer-wasix --features sys` |
| CLI tests | `make test-wasmer-cli` |
| Examples | `make test-examples` |
| C API | `make test-capi` |

- WASIX guest-side tests live in `lib/wasix/tests/wasm_tests/`. They need
  `wasixcc` and `cargo-wasix` on PATH. Fixture directives are documented in
  `lib/wasix/tests/wasm_tests.rs`.
- If a test fails only on one backend or platform, add a line to
  `tests/ignores.txt` with a reason. Do not chase Singlepass failures for
  SIMD, relaxed SIMD, exception handling, or wide arithmetic. Singlepass
  does not support these proposals.
- To debug a failing WAST test, find the `.wast` file under `tests/wast/`.
  Then rerun with the backend-filtered command. The runner prints
  `Running wast <path>`.

CAUTION: `make test-wast` uses `cargo test`, not nextest, because the tests
must share one process. Do not convert it to nextest.

## Lint and format

CI rejects lint and format failures. Run before you push:

```bash
make lint
```

This runs `cargo fmt --all -- --check`, clippy with `-D clippy::all` plus a
RUSTFLAGS deny list, `taplo format --check`, `yamlfmt -lint .github`, and
`clang-format`. To apply clippy fixes in bulk:

```bash
cargo clippy --all --exclude wasmer-swift --locked --fix --allow-dirty -- -D clippy::all
```

`cargo fmt` does not cover `lib/wasix/tests/wasm_tests`. CI formats those
files with `rustfmt --edition 2024` directly.

## Debug the runtime

- Log filter: `RUST_LOG` with standard EnvFilter syntax, for example
  `RUST_LOG="warn,wasmer_wasix=trace"`. The `-v` to `-vvvv` flags map to
  warn/info/debug/trace for the targets `wasmer`, `wasmer_wasix`, and
  `virtual_fs`. Implementation: `lib/cli/src/logging.rs`.
- Run the workspace CLI directly:
  `cargo run -p wasmer-cli --features cranelift -- run file.wasm`.
- Inspect Wasm with `wasm-tools print file.wasm` (pipe through `head` or
  `grep` — output is large). Author test cases with
  `wasm-tools parse file.wat -o file.wasm`. Reduce hard cases with
  `wasm-opt` and `creduce`.
- Compile a C repro to WASIX:
  `WASIXCC_WASM_EXCEPTIONS=1 WASIXCC_PIC=1 wasixcc -g -O0 file.c -o file.wasm`.

## Submodules

Three submodules: `tests/wast/spec`, `lib/napi`, `wasmer-test-files`.
Initialize with `git submodule update --init --recursive`.

CAUTION: `git status` often shows the submodules as modified. Do not commit
a submodule pointer bump in an unrelated PR. Spec syncs are explicit
commits, for example `test: sync SPEC test submodule (#6827)`.

## Commits and PRs

- Title format is conventional-commit style with crate or area scopes:
  `fix(Singlepass): ...`, `feat(wasix): ...`, `feat!: ...` for breaking
  changes, `chore: ...`, `deps: ...`.
- PRs are squash-merged to `main`. The PR template has one `# Description`
  section. For large changes, open a GitHub issue first.
- Before you submit: `make lint`, then `cargo test` in the crates you
  touched, then a build of the CLI.
- Do not hand-edit `CHANGELOG.md` or crate versions. Releases generate
  both (`scripts/make-release.py`, `scripts/update-version.py`).
- macOS and musl CI jobs run on a PR only when the PR has the `macos` or
  `musl` label. If your change touches platform code, add the label.
- Windows builds use `--no-default-features --features v8`. The native sys
  backend is not supported on Windows. no-std support is removed, but the
  `check-compilers-only-std` and `check-baremetal` CI jobs still gate.

WARNING: Do not fix a security vulnerability in this public repository.
Embargoed work goes through the private process in `docs/SECURITY.md`.
A public fix before disclosure exposes users.

## Gotchas

- LLVM at any version other than 22 silently disables the LLVM backend.
  Read the `Enabled Compilers:` banner.
- Full release builds with LLVM take tens of minutes and `target/` grows
  to multiple GB. Prefer `make check` and `-p <crate>` builds to iterate.
- The wasix-libc sysroot and Rust toolchain pins for CI live in
  `.github/ci-constants.env`.
- macOS and musl coverage gaps: a PR can pass CI and still break `main`
  on those platforms.
