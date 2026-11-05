# WASIX directory migration and legacy removal gates

The machine implementation now lives under `lib/wasix/`, retaining its
nested crates and existing `wasixv2*` package names. The old `wasmer-wasix`
crate moves intact to `lib/wasix-legacy/`. This is a directory migration,
not evidence of functional parity or completed legacy removal.

No legacy crate is deleted. All six crates below still have live consumers
or provide their public APIs. A replacement must preserve the behavior of
each consumer before its old implementation can be removed. A successful
Cargo check alone does not establish that behavior.

## Crate map

| Retained legacy crate | Existing responsibility and consumers | V2 implementation and current evidence | Removal gate |
| --- | --- | --- | --- |
| `wasmer-wasix` (`lib/wasix-legacy`) | Guest imports, `WasiEnv`/`WasiEnvBuilder`/`WasiFunctionEnv`, pluggable runtime, package acquisition/resolution/caches, HTTP, runners, hooks, journal integration. Public exports are in [`src/lib.rs`](../lib/wasix-legacy/src/lib.rs); consumer details follow below. | [`Machine`](../lib/wasix/api/src/lib.rs), [`Package`](../lib/wasix/api/src/package.rs), [`exec`](../lib/wasix/exec/src/lib.rs), [`kernel`](https://github.com/wasmerio/wlk/blob/main/crates/kernel/src/lib.rs), and [`wasix1`](../lib/wasix/wasix1/src/lib.rs) provide new execution machinery. Existing tests cover [machine execution](../lib/wasix/api/tests/machine.rs), [packages](../lib/wasix/api/tests/package.rs), [commands](../lib/wasix/api/tests/commands.rs), [custom bindings](../lib/wasix/api/tests/custom_bindings.rs), and [v1 guests](../lib/wasix/wasix1/tests/guest.rs). These are coverage locations, not a claim of full old API or behavior parity. | Port every consumer listed below; preserve source compatibility or obtain explicit approval for each removed public API. Registry acquisition, credentials, persistent caches and the old embedding facade are still used. |
| `wasmer-wasix-types` (`lib/wasi-types`) | Published v1 ABI types, asyncify layouts, serde/rkyv support; used by the legacy runtime and `wasmer-journal`, and re-exported by the legacy public facade. See [`src/lib.rs`](../lib/wasi-types/src/lib.rs) and [`Cargo.toml`](../lib/wasi-types/Cargo.toml). | The [v1 adapter ABI](../lib/wasix/wasix1/src/abi/mod.rs) decodes old guest layouts; [`wasixv2-abi`](https://github.com/wasmerio/wlk/blob/main/crates/abi/src/lib.rs) defines the new ABI. Adapter [layout tests](../lib/wasix/wasix1/tests/layout.rs) cover its layouts. | Internal guest-layout decoding does not replace the published Rust types or serialized journal representations. Retain until both runtime and journal consumers are replaced and public compatibility is settled. |
| `virtual-fs` (`lib/virtual-fs`) | Public `FileSystem`/`VirtualFile` traits, buffers/pipes, host/memory/overlay/mount/WEBC filesystems and adapters. Used by legacy runtime, journal, CLI, Swift and NAPI's WASIX integration. See [`src/lib.rs`](../lib/virtual-fs/src/lib.rs). | [`wasixv2-vfs`](https://github.com/wasmerio/wlk/blob/main/crates/vfs/src/lib.rs), `vfs-mem`, `vfs-host`, `vfs-overlay`, `vfs-package`, `vfs-web`, and host byte streams cover corresponding new machinery; [v1 filesystem tests](../lib/wasix/wasix1/tests/fs.rs) exercise guest translation. | Port custom Rust filesystem/file implementors and all stream/capture APIs, then demonstrate equivalent consumer behavior. The new filesystem interfaces are different types. |
| `virtual-net` (`lib/virtual-net`) | Public virtual socket/network traits; host, loopback, composite and remote client/server networking; ruleset parser. Runtime and journal use its types. CLI v2 admission still wraps [`ruleset::Ruleset`](../lib/virtual-net/src/ruleset.rs); CLI TUN uses virtual sockets. | `vnet`, `vnet-host`, `vnet-switch`, `vnet-web` implement new transport paths. See [machine networking tests](../lib/wasix/api/tests/net.rs) and [v1 socket tests](../lib/wasix/wasix1/tests/net.rs). | Port or retain the CLI ruleset parser and TUN behavior; map custom networking implementors, remote protocol, serialization and journal address types. A browser relay does not establish old remote-protocol compatibility. |
| `virtual-mio` (`lib/virtual-io`) | Interest handlers, wakers, selector and registration guards; used by virtual-fs, virtual-net, legacy runtime and CLI TUN. See [`src/lib.rs`](../lib/virtual-io/src/lib.rs). | [`Host`/`Executor`](https://github.com/wasmerio/wlk/blob/main/crates/host/src/lib.rs), [`TokioHost`](https://github.com/wasmerio/wlk/blob/main/crates/host/src/tokio_host.rs), and [browser drivers](https://github.com/wasmerio/wlk/blob/main/crates/host/src/browser/mod.rs) provide the new scheduling/reactor boundaries. | Port every old `InterestHandler`/`Selector` consumer and its wakeup/cancellation behavior. Existing host abstractions are not source-compatible substitutes. |
| `wasmer-journal` (`lib/journal`) | Existing serialized journals, readers/writers, filters, compaction and snapshot records; used by legacy runtime and `wasmer journal`. See [`src/lib.rs`](../lib/journal/src/lib.rs). | `wasixv2-record` and `wasixv2-checkpoint` are separate new mechanisms. No journal-format or snapshot compatibility claim is made. | Snapshot parity is excluded from this migration. Preserve old journal commands, serialized data support and source until separately authorized replacement/removal. |

The new runtime crates retained under `lib/wasix/` are exactly:

```text
wasixv2                 (api)
wasixv2-abi             wasixv2-checkpoint      wasixv2-dylink
wasixv2-exec            wasixv2-host            wasixv2-kernel
wasixv2-pages           wasixv2-policy          wasixv2-record
wasixv2-stdio-web       wasixv2-syscalls        wasixv2-trace
wasixv2-vfs             wasixv2-vfs-host        wasixv2-vfs-mem
wasixv2-vfs-overlay     wasixv2-vfs-package     wasixv2-vfs-web
wasixv2-vnet            wasixv2-vnet-host       wasixv2-vnet-switch
wasixv2-vnet-web        wasixv2-wait            wasixv2-wasix1
```

`lib/wasix/codegen` is the unchanged ABI/code-generator submodule. Its
`wasixv2-codegen` package is retained too. `wasixv2-wasix1` is the active
WASIX v1/WASI guest adapter on the new kernel; it is not the old runtime
and must not be removed as part of legacy retirement.

## Consumer gates

| Consumer | Source-backed remaining use | Required behavior before removal |
| --- | --- | --- |
| CLI execution and packages | [`run/machine.rs`](../lib/cli/src/commands/run/machine.rs) runs guests on `Machine` but still uses old package/runtime loaders, HTTP client and `virtual_net::ruleset`. [`run/package_source.rs`](../lib/cli/src/commands/run/package_source.rs), [`run/target.rs`](../lib/cli/src/commands/run/target.rs), [`run/runtime.rs`](../lib/cli/src/commands/run/runtime.rs), and [`package/tree.rs`](../lib/cli/src/commands/package/tree.rs) use old resolver, package loading, module caches and monitoring. | Preserve registry/file/URL package resolution, dependency validation, credentials, caches, logging, compiler selection, HTTP, network rules and command annotations. The new byte-backed `Package` deliberately leaves acquisition/credentials/caches to its embedder. |
| CLI journal | [`commands/journal`](../lib/cli/src/commands/journal) imports `wasmer_wasix::journal` for extract/export/import/inspect/compact/filter. | Retain old command behavior and data formats; snapshot parity is outside this change. |
| C API | [`wasm_c_api/wasi/mod.rs`](../lib/c-api/src/wasm_c_api/wasi/mod.rs) builds `WasiEnv`, stores `WasiFunctionEnv`, supplies virtual files and creates imported memory through old `SpawnType`. | Preserve exported C symbols, memory ownership/lifetime, arguments/environment/preopens, import generation, initialization, execution, and captured streams. Updating a Cargo path preserves existing behavior; it does not port this API. |
| `wasmer-c-api-imports` | [`src/wasix.rs`](../lib/c-api-imports/src/wasix.rs) implements old `InstantiationHook` using `InstantiationState`. | Adapt real C API guest bindings to the new hook's import preparation, memory reuse, early start, instance attachment and cleanup, and test execution/fork/exec failure paths. Generic new [custom-binding tests](../lib/wasix/api/tests/custom_bindings.rs) do not prove this consumer port. |
| NAPI | The pinned [`lib/napi/Cargo.toml`](../lib/napi/Cargo.toml) and `src/wasix.rs` use the old facade and hooks; its async WASIX feature also uses `virtual-fs`. | Integrate the separately developed NAPI facade without overwriting its source; preserve guest import behavior, memory, per-instance state, threads, fork/exec and cleanup. Its old dependency path becomes `../wasix-legacy`; new facade dependencies target `../wasix/api`. This migration includes a two-line manifest-only NAPI commit (`afe139d`) for those paths; merge it into the ongoing facade branch before final integration, preserving that branch's source. |
| Swift | [`lib/swift/src/lib.rs`](../lib/swift/src/lib.rs) builds old `WasiEnv`, uses `ArcFile`/`BufferFile` capture and old run/error APIs. | Preserve Swift-facing execution, errors and stdout capture with the new machine, then remove old dependencies. |
| Root meta-crate/examples | Root [`Cargo.toml`](../Cargo.toml)'s `wasi` feature selects `wasmer-wasix`; [`wasi.rs`](../examples/wasi.rs), [`wasi_manual_setup.rs`](../examples/wasi_manual_setup.rs), and [`wasi_pipes.rs`](../examples/wasi_pipes.rs) compile against its embedding APIs. | Preserve the feature and examples until an explicit source-compatible facade or approved consumer migration exists. |
| `wasmer-sys-utils` | [`Cargo.toml`](../lib/sys-utils/Cargo.toml) declares `wasmer-wasix.workspace`; the only source mention is commented code in [`tests/fd_mmap_memory.rs`](../lib/sys-utils/tests/fd_mmap_memory.rs). | This dependency can be audited for separate removal; removing an unused declaration does not establish parity of the legacy runtime. |
| Browser SDK | Browser machine/transport primitives live in [`api/src/browser.rs`](../lib/wasix/api/src/browser.rs), [`host/src/browser`](https://github.com/wasmerio/wlk/blob/main/crates/host/src/browser), and web drivers. The SDK consumer is maintained outside this checkout. | Update SDK path dependencies from `lib/wasixv2/<crate>` to `lib/wasix/<crate>` when integrating its ongoing transport work; preserve registry/cache/streams and real browser/worker behavior. No SDK source is overwritten here. |

## Evidence preservation and validation boundaries

All 744 tracked files under the old `lib/wasix/tests/` are retained at
`lib/wasix-legacy/tests/`, including old fixtures, expectations, harnesses
and runner code. The harness still resolves its fixtures from
`CARGO_MANIFEST_DIR/tests/wasm_tests`; its directory depth is unchanged.
Only the test README's direct file paths change. Future old/new comparisons
must reuse these fixtures and record each result before deleting anything.
The existing new-kernel v1 tests remain under `lib/wasix/wasix1/tests/`.

The directory migration does not complete the deferred WASIX64 or Windows
work. Deferred work and excluded snapshots are not approval to silently
remove their old implementation. Existing source, fixtures, crate names,
versions and feature names remain available.

The Cargo workspace still names the old runtime explicitly and includes
the new crates with `lib/wasix/*`. Existing code-generator gitlink identity
and pinned revision are unchanged; only its path changes. Generated source
headers now name the new WIT location. A WIT-derived comment in the generated
syscall file still names the historical `lib/wasixv2/DESIGN.md` path because
the upstream ABI source and pinned revision were not changed.

In an existing checkout, synchronize and update submodules after changing
branches:

```sh
git submodule sync --recursive
git submodule update --init --recursive
```

Use the same named-package build/test commands as before; public package
identities did not change. Run legacy guest tests with `cargo test -p
wasmer-wasix --features sys`, new adapter tests with `cargo test -p
wasixv2-wasix1`, the generated-source checks with `cargo test -p wasixv2-abi
--test generated`, and the CLI/C API/NAPI/Swift consumer checks after their
respective ports. These commands are requirements for the relevant change,
not a record of tests already run by this migration.

## Directory migration checks

The migration passed locked workspace metadata (64 members, including the
25 nested runtime crates, unchanged code-generator package and six legacy
crates), locked CLI dependency resolution, browser dependency isolation,
`cargo fmt --all -- --check`, shell syntax checks for the relocated browser
scripts, and 117 existing Node tests across the facade, host, execution
memory and stream drivers. A git-blob comparison verified that all legacy
test fixtures and harnesses are unchanged apart from the README paths.

Compiler builds, Rust execution tests and the full `make lint` gate have
not been run for this path-only change while compilation slots are reserved
for runtime work. These checks do not establish legacy removal parity.
