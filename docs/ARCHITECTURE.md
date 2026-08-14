# Repository architecture

This document maps the workspace: which crate owns which concern. One place
per concern — if two crates seem to own the same thing, one of them is the
wrong place for your change.

| Path                                                  | Content                                                              |
| ----------------------------------------------------- | -------------------------------------------------------------------- |
| `lib/api`                                             | Public `wasmer` crate. Flavors: `sys` (native, default), `v8`, `js`. |
| `lib/cli`                                             | CLI crate `wasmer-cli`. Binaries: `wasmer`, `wasmer-headless`.       |
| `lib/compiler`                                        | Compiler framework and traits.                                       |
| `lib/compiler-{singlepass,cranelift,llvm}`            | The three compiler backends.                                         |
| `lib/vm`                                              | Low-level VM runtime.                                                |
| `lib/wasix`                                           | The WASIX implementation (syscalls, processes, threads).             |
| `lib/journal`                                         | Snapshot and restore support. See [journal.md](./journal.md).        |
| `lib/virtual-fs`, `lib/virtual-net`, `lib/virtual-io` | Host abstraction layers.                                             |
| `lib/c-api`                                           | C API (`cdylib` named `wasmer`).                                     |
| `lib/backend-api`                                     | GraphQL client for the Wasmer registry.                              |
| `lib/config`, `lib/package`                           | `wasmer.toml` parsing and webc packaging.                            |
| `tests/compilers`                                     | Integration tests (`wast.rs`, `traps.rs`, `issues.rs`, more).        |
| `tests/wast/{spec,wasmer}`                            | WAST test files. `spec` is a submodule.                              |
| `tests/ignores.txt`                                   | Per-compiler test skip list: `<compiler> <test> # reason`.           |

The root crate is a meta-crate. The root `build.rs` generates one WAST spec
test per compiler from `tests/wast/spec`.

## Submodules

Three submodules: `tests/wast/spec`, `lib/napi`, `wasmer-test-files`.
Initialize with:

```bash
git submodule update --init --recursive
```

`git status` often shows the submodules as modified. Do not commit a
submodule pointer bump in an unrelated PR. Spec syncs are explicit commits,
for example `test: sync SPEC test submodule (#6827)`.
