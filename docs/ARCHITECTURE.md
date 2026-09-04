# Repository Architecture

This document maps the workspace, one crate per concern. The workspace
holds tens of crates; the full listing is the `members` section of the
root [Cargo.toml](../Cargo.toml). The table below covers the paths you
will touch most:

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

The root crate is a meta-crate.

## Submodules

Three submodules: `tests/wast/spec`, `lib/napi`, `wasmer-test-files`.

For initialization and working with the different crates + submodules, see [BUILD.md](./BUILD.md)
