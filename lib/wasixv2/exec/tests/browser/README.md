# Browser foreign memory (C1)

`ForeignMemory` keeps the existing synchronous `SharedMappings::attach_memory`
contract. Creation posts a registration before attachment, with no acknowledgement.
A worker with a local shared-memory handle uses that JS Memory directly; otherwise
it waits on its own runtime-TLS request slot for a dedicated pure-JS copy worker.
The service copies directly between guest memory and the borrowed Rust buffer in
the shared runtime heap. It never loads runtime wasm, calls Rust, or obtains any
kernel, store, allocator or retained-page lock. Linux access/retention authorities
are `mm/memory.c:__access_remote_vm`,
`mm/process_vm_access.c:process_vm_rw_core` and `io_uring/rsrc.c`.

## Embedding

Use B2's `startCoordinator`, passing these additional options:

```js
memoryCopyUrl: new URL('./memory/copy-worker.js', import.meta.url).href,
memoryCopyTimeout: 10000,
```

Serve `copy-worker.js`, `client.js` and `operations.js` together at that location.
`memoryClientUrl` can override the sibling client URL. The coordinator alone
creates the copy worker, connects one MessagePort per runtime worker, and terminates
it on normal shutdown or fatal failure. The copy worker uses no runtime worker,
TLS allocation, service admission slot, wasm-bindgen module or object envelope.
The existing API object hooks must still be installed to deliver engine objects
at task admission; C1 uses their worker-local registry for its fast path.

No kernel work may execute on the page main thread. `installMemoryClient` rejects
that context and requires a shared runtime heap. The primary no-JSPI worker lane
blocks only dedicated runtime workers; neither the page nor the target worker
must handle an event to complete a foreign copy.

## Ordering and failure

Registrations use never-reused u64 IDs (JS BigInt). Final Drop posts unregister,
including when it runs on a worker other than the registering worker. The service
holds requests for unknown IDs until registration arrives; cross-port unregister
before registration discards that later registration rather than retaining a dead
memory. A live Rust handle keeps the guest memory alive through every copy.

Unregister removes the service's Memory reference, but the engine can keep a
dead wrapper's address reservation until that isolate collects. After eight
unregistrations the coordinator replaces this pure-JS realm. The old realm
hands its live registry, retirement markers, pending slot offsets and client
endpoints directly to the replacement through a private MessageChannel; queued
client messages remain ordered. The receiver first announces ownership on that
channel. The sender stays alive until the coordinator receives restored-state
readiness, which avoids Firefox shared-memory decode failures during handoff.
These service bootstrap acknowledgements do not acknowledge C1 registrations.
After acknowledgement and normal drained runtime shutdown, the pure-JS realm
closes its own client/handoff ports and then itself. Fatal/uncertain heap failure
still forcefully terminates the service and every Rust worker.
No runtime Rust, forced GC, guest progress or new kernel contract is involved.

One atomic slot per runtime worker publishes operation, ID, guest offset, runtime
buffer address/length and CAS arguments. Release/acquire completion precedes any
return into Rust. Growth refreshes both guest and runtime buffer views. The caller
checks PageAccess, low guard and read-only ranges before dispatch; the service
checks current bounds. Forced copies retain the same exceptions as local copies.
There is no atomic snapshot of racing guest bytes, as on Linux remote access.

A service error or timed wait is a B2 runtime `fatal`, never EFAULT. On timeout the
caller marks the pending slot aborted, posts fatal and parks until termination.
It must **never return or throw through guest handlers into Rust**: the service
could still be copying into the borrowed buffer, and Rust must not free or reuse
it. Even failure to post fatal cannot return into Rust. B2 discards all runtime
workers and the copy service; the embedder must fail every Machine attached to
that runtime, following B2's existing fatal contract. No poisoned heap is reusable.

## Reproduce

Use the same pinned nightly/rust-src and matching wasm-bindgen CLI as B2. The build
reuses B2's isolated std TLS cleanup recipe, structural checks and headless runner:

```sh
export CARGO_TARGET_DIR="$HOME/wasixv2-orch/logs/b3b-foreign/target"
export WASM_BINDGEN=/path/to/wasm-bindgen
lib/wasixv2/exec/tests/browser/build.sh
nice -n 19 taskset -c 4-7 node lib/wasixv2/host/tests/browser/run.mjs "$CARGO_TARGET_DIR/browser"
nice -n 19 taskset -c 4-7 node --test \
  lib/wasixv2/exec/tests/browser/memory.test.mjs \
  lib/wasixv2/host/tests/browser/protocol.test.mjs
```

The test-artifact-only `browser-memory-test` feature builds the exec sources as a
cdylib example and includes its regression exports. Embedders enable `js` only.
All generated wasm/assets remain in CARGO_TARGET_DIR and must not be committed.

The real isolated Chromium page forces JSPI off. It exercises local and genuinely
undelivered foreign handles while a source-built WAT guest runs and its worker
parks, immediate retained-callback completion, guest growth, bounds/overflow/empty
copies, PageAccess/guard/read-only/forced parity, CAS, zeroing, retention after store
and owner drop, idle worker reuse and a silent-service timeout that fails the
runtime. The page heartbeat remains live. Node tests cover unknown full-width IDs,
registration/unregister ordering, aborted requests, direct runtime/guest growth,
coordinator creation, service death, normal teardown and page rejection.

This is the C1 data-path gate, not full facade/SDK signoff. B8 must supply the worker
asset URLs and route B2 fatal notifications to Machine waiters. End-to-end kernel
ptrace/process_vm/io_uring, combined B3a/B10 integration and additional browsers
belong to the integrated browser harness lane. No full wasixv2 suite is needed.
