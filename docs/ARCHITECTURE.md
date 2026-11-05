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
| `lib/wasix/*`                                         | Wasmer-specific WASIX facade, execution, loader and v1 bindings.      |
| `lib/wasix-legacy`                                    | Legacy `wasmer-wasix` embedding and package APIs; retained until consumer parity. |
| `lib/journal`                                         | Snapshot and restore support. See [journal.md](./journal.md).        |
| `lib/virtual-fs`, `lib/virtual-net`, `lib/virtual-io` | Host abstraction layers.                                             |
| `lib/c-api`                                           | C API (`cdylib` named `wasmer`).                                     |
| `lib/backend-api`                                     | GraphQL client for the Wasmer registry.                              |
| `lib/config`, `lib/package`                           | `wasmer.toml` parsing and webc packaging.                            |
| `tests/compilers`                                     | Integration tests (`wast.rs`, `traps.rs`, `issues.rs`, more).        |
| `tests/wast/{spec,wasmer}`                            | WAST test files. `spec` is a submodule.                              |

The root crate is a meta-crate. The WASIX crate identities are unchanged by
the directory migration; see [the consumer parity gates](./WASIX-MIGRATION.md)
for the legacy crates that still need replacements before removal.

## Submodules

Five submodules: `tests/wast/spec`, `lib/napi`, `wasmer-test-files`,
`tests/wasixv2` ([wasixv2-tests](https://github.com/wasix-org/wasixv2-tests), the
wasixv2 test suite, run with `make test-wasixv2`), and `lib/wasix/codegen`.
The last is
[wasixv2-abi](https://github.com/wasix-org/wasixv2-abi): the wasixv2 guest ABI in
WIT and the `wasixv2-codegen` generator, shared with libc2. ABI changes land
there first; `lib/wasix/exec/tests/generated.rs` checks that the generated
wasixv2 crates match the pinned commit.

For initialization and working with the different crates + submodules, see [BUILD.md](./BUILD.md)

## Engine-independent kernel

The kernel, syscall traits/layouts, scheduling primitives, host services, VFS/VNet
and their drivers live in the **private** [wasmerio/wlk](https://github.com/wasmerio/wlk)
workspace under `crates/`. Wasmer consumes its `main` branch via centralized git
dependencies; `Cargo.lock` fixes the exact revision. Crate names are unchanged.
Only the Wasmer-specific `api`, `exec`, `dylink`, `wasix1` and canonical `codegen`
remain in `lib/wasix`. The generator checks for both repositories run from
`cargo test -p wasixv2-exec --test generated`; `WLK_ROOT` selects a writable kernel
checkout when intentionally blessing generated kernel outputs. Kernel behavior tests
run in WLK; actual engine/guest and v1 reference-protocol tests remain here.
