# WASIX v2 browser support: audit, design and work packages

Design proposal, 2026-10-30. No runtime implementation or tests were performed for this plan.
The goal is a browser runtime suitable for porting the separate wasmer-sdk project, with at
least the v1 feature surface and quality. A successful `cargo check` alone is not that goal.

## 1. Evidence and scope

Audited Wasmer at `feat/wasixv2` **f2e457967c8e4755bd466c9822fbaa5f20c1ad73**,
ABI **07cd32edb66b0cf9f97e276b85e0398c899daa5c**, tests pin **a0634b3**; libc2
**b969f9aa** is the owner's stated baseline, not independently built here. This document is
on `wasixv2/plan-browser`, based on `origin/wasixv2/plan` **8f3b073887d**. Implementation
branches must start from the integration branch, not this older plan tree.
`lib/wasixv2/PLAN.md` does not exist at the audited integration revision; the plan-branch
copy was read instead. DESIGN's browser-build claim predates the failures recorded below.

Reference snapshots, read from source:

- [Current wasmer-sdk](https://github.com/wasmerio/wasmer-sdk/tree/c352b08566e1564362c747758e3fe440e9d01a33),
  **c352b08566e1564362c747758e3fe440e9d01a33**: `js/src`, `js/bindgen/src`, `rust/src`.
- [Classic JS SDK](https://github.com/wasmerio/wasmer-sdk/tree/94354975f119391cf1cdb4adf97a7190c7e2960d),
  tag `wasmer-sdk-v0.6.0`, **94354975f119391cf1cdb4adf97a7190c7e2960d**:
  `src`, `WasmerSDK.ts`, `.cargo/config.toml`. This is the original `Wasmer.fromRegistry`,
  `runWasix`, `Directory`, `Runtime`, `Instance` API.
- [Older wasmer-js](https://github.com/wasmerio/wasmer-js/tree/5c9a42f60fda0f48e731bf685746568db362392c),
  tag `v0.9.0`, **5c9a42f60fda0f48e731bf685746568db362392c**: `packages/wasi`,
  `wasmfs`, `wasm-terminal`, `io-devices`. Its default URL now resolves to the same current
  history as SDK, so auditing HEAD twice would miss this implementation.
- Linux checkout **d135ab453a7400a34930bdda1ef4bdc8acfac80e**, at
  `~/wasixv2-tools/linux`. Linux file/function references below name the semantic authority;
  browser transport choices are our design, not Linux behavior.

The owner's explicit request authorizes reading v1 for this audit, overriding the older
`plan/rules.md` prohibition. Reuse the existing v2 kernel semantics and v2 abstractions;
do not import v1 syscall policy into the kernel. No work here changes dynamic linking,
closures, libffi, reflection, guest ABI, or submodule pins. Dependencies on the excluded
team are reported, not worked around in their files.

## 2. V1 browser capability inventory

“Current” means the SDK snapshot above; “classic” means the original SDK snapshot. This
inventory includes public configuration and lifecycle operations as well as guest features.
SDK UI, registry credentials, distribution, and browser service-worker hosting remain in SDK.

| Feature exposed to JS / guest | SDK implementation and public surface | This repository's v1 implementation | V2 destination / parity obligation |
| --- | --- | --- | --- |
| Raw module execution and WAT conversion | Classic `src/run.rs:run_wasix`, `src/lib.rs:wat2wasm`; current `packages.load(bytes)` accepts raw `_start` modules | `lib/api` JS engine; `lib/wasix` builder/environment and runners; `wat` utility | `Machine::run_module/run_process`, exec ABI registry; SDK retains conversion helpers |
| Registry, local WEBC, named commands, entrypoint, command binary | Classic `src/wasmer.rs:Wasmer::{js_from_registry,js_from_file}`, `Command::{run,binary}`; current `Packages::{create,load,loadMany}`, `Package::command`, command references | `wasmer-package`, `webc`, `lib/wasix/src/runtime/{resolver,package_loader,module_cache}`, `bin_factory`, `runners` | Existing v2 package/overlay drivers, `install_commands`, loader and `run_file`; preserve command annotation translation in wasix1 |
| args, env, cwd, additional packages | Classic `src/options.rs`, `uses` injection; current sandbox env, command args/env/cwd, installPackage, shell and tagged shell template | `WasiEnvBuilder`, package resolver/runner and binary factory | `ProcessConfig`, mount/install commands; guest `execve` remains VFS-based; package acquisition is embedder work |
| Multiple instances, shared runtime/cache/pool | Classic `Runtime::global`, explicit `runtime`; current `Wasmer`, sandboxes, per-client peer networks, `close`, `shutdown`, async disposal | `PluggableRuntime`, module cache, task-manager trait | One Machine per sandbox, shared browser host/pool if desired; distinct kernel object graphs; teardown drains all owned tasks |
| stdin, stdout, stderr; wait and captured output | Classic `Instance` Web Streams, `Instance.wait` drains both outputs and joins exit; `src/streams.rs` | `virtual-fs::Pipe`, static files; v1 fd operations | `ByteSource/ByteSink`, bounded queues, kernel pipes where pipe semantics are required; EOF and drain before final process result |
| Live processes, capture/pipe/discard, input close, timeouts, terminate/kill | Current `Command::{run,spawn}`, `Process::{wait,terminate,kill}`, byte iterators, lines, Web Stream adapters, capture limits/truncation flags | SDK `rust/src/{process,stream,capture,command}.rs` on v1 process/thread handles and pipes | SDK API stays separate; v2 run future, signaller and kernel process tree implement lifecycle; resource failure must resolve every waiter |
| TTY, raw input, echo, line editing, Ctrl-C, size changes | Classic `src/wasmer.rs:setup_tty/copy_stdin_to_tty`; current `terminal`, `resizeTerminal`, `rust/src/terminal.rs` | `lib/wasix/src/os/tty.rs`, Tty/TtyOptions, v1 `tty_get/set`; `virtual-fs` pipes | Existing v2 console, n_tty and ptys; browser supplies raw bytes and winsize, no second JS line discipline |
| Mutable in-memory directories and mounted directory instances | Classic `Directory` readDir/writeFile/readFile/readTextFile/createDir/removeDir/removeFile; `mount` maps guest paths to shared directory instances | `lib/virtual-fs/src/{mem_fs,tmp_fs,overlay_fs,...}`, mount composition, package files | `wasixv2-vfs-mem`, VFS mounts and bind mounts, overlay and package; shared mount object identity and data survive process exit |
| Filesystem editing after sandbox creation | Current `SandboxFileSystem` read/write text/bytes, mkdir, readDir, stat, remove, rename; package definitions include files | SDK provider filesystem over `virtual-fs` | SDK's wrapper calls v2 VFS with explicit credentials/root; no host path string traversal |
| Host-provided mounts/storage | Current `js/bindgen/src/host_filesystem.rs`, `js/src/host-filesystem.ts`, mountHost/storageHost bridge; documented experimental native-host protocol | `virtual-fs::FileSystem` provider in SDK, not browser access to arbitrary OS paths | Browser provider implementing v2 Filesystem/Node/Handle, plus memory/package mounts; File System Access/OPFS require explicit capabilities, no promise of native inode behavior |
| Guest pthreads and worker concurrency | Classic and current `tasks/{thread_pool,scheduler,task_wasm,worker_handle,thread_pool_worker}`, configurable worker URL; current parallelism and worker retirement | `lib/wasix/src/runtime/task_manager/mod.rs:VirtualTaskManager`, task/thread/process control; `lib/api` JS module/shared-memory transfer | Existing `Executor::spawn_thread(ThreadStart)` plus browser pool; one executing guest task per worker, shared Rust kernel heap, separate guest address spaces |
| Sleep, polling, futexes, atomic waits | Classic task-manager sleep and worker execution; current `tasks/timer.rs`, dedicated timer worker and `sleep_now`; synchronous waits may park a task worker | v1 task-manager sleep, futex/poll syscall machinery; shared Rust std uses wasm atomics when rebuilt for threads | Host timer driver, existing WaitQueue/futex and AtomicWait hooks; Atomics wait/notify parks workers, not an independent guest futex namespace |
| SharedArrayBuffer / worker bootstrap | SDK runtime itself is a threaded wasm module; classic `.cargo/config.toml`, current `js/scripts/build-wasm.mjs`, worker JS init and TLS | `lib/api/src/backend/js/utils/shared_handle.rs`, `js::{prepare_shared_object_message,receive_shared_object_message}`, shared Memory/Module handles | Reuse these engine facilities, match runtime build/TLS initialization, never send JS slab indices as cross-worker references |
| Outbound TCP, UDP and resolver through a websocket gateway | Classic `RuntimeOptions.networkGateway`, `src/net.rs`, `src/ws.rs` | `lib/virtual-net/src/{client,server,meta,rx_tx}.rs`: RemoteNetworkingClient/Server, requests/replies; optional websocket server transport; server ultimately opens host sockets | New browser implementation of v2 Backend/BackendStream/BackendDatagram using the established relay wire protocol; guest routes and ports remain v2-owned |
| WISP egress and endpoint replacement | Current `network: wisp`, requestUrl, setWispUrl, `js/src/wisp-network.ts`, `node-network-rpc.ts`, `js/bindgen/src/node_network.rs` | SDK adapter to v1 VirtualNetworking; WISP and DNS-over-HTTPS are SDK browser transports, not the v1 remote bincode protocol | Pluggable transport under v2 Backend; retain classic relay TCP/UDP parity and current WISP TCP/DNS, do not conflate protocols or silently lose UDP |
| Loopback, sandbox peer TCP, browser HTTP ingress | Current `network: http/wisp` peer configuration; `Ports::{wait,expose,onListen}`, BrowserServer/iframe; service-worker and direct TCP bridge | SDK `browser_http.rs`, `browser_tcp.rs`, service-worker TS; virtual sockets present HTTP request bytes as a TCP stream to the guest | V2 local Net and vnet-switch; ingress via InboundSink/Publication; HTTP framing/service worker remains SDK; port discovery needs a small read-only embedding hook if none suffices |
| Host fetch-based HTTP client | Classic Runtime HTTP client, current runtime package HTTP loading | `lib/wasix/src/http/web_http_client.rs:WebHttpClient`, web `fetch` on Window/WorkerGlobalScope; current helper can run on another task to avoid blocking its JS event loop | Fetch stays host-side for registry/download; v1 `wasix_http_client_v1` compatibility, currently missing in v2, belongs only in wasix1 with an embedder provider |
| Registry URL/token, caching, progress/cancellation | Classic RuntimeOptions.registry/apiKey and package loader; current cache namespace/readOnly, progress phases, AbortSignal, deduplicated loads | BackendSource GraphQL requests, WebHttpClient, BuiltinPackageLoader, package/module caches; SDK CacheStorage adapters | SDK owns auth, Fetch, CORS and persistent cache; repo must accept byte-backed webc/modules and async loader without native filesystem/network defaults |
| Feature probing, logging and bundling | Classic init, global internals, setWorkerUrl, tracing; current Wasmer.create, worker URLs, module/package cache | JS backend capabilities and wasm-bindgen; WASIX `js` selects web clocks/getrandom/memfs, avoids native sys-thread | Explicit browser host factory, capability report, versioned worker module/asset contract; keep UI/bundler API in SDK |

V1 worker support is split: the generic runtime/task-manager interface lives in this repo;
the actual browser scheduler and worker bootstrap live in SDK. `lib/wasix`'s concrete Tokio
manager is not a browser worker manager. Likewise, relay socket APIs are not browser-native
TCP/UDP, and browser-mounted memory directories are not unrestricted native directories.
The old TS wasmer-js stack used memfs, browser clocks/randomness, xterm/Comlink process
workers, WAPM fetching and synchronous input fallbacks; it is useful historical evidence,
not a source of Linux kernel semantics or a required resurrection of its prompt-based stdin.

## 3. V2 compile audit

### Reproduction and observed results

A separate detached audit worktree was created at the pinned integration revision. Only
`lib/napi` and `lib/wasixv2/codegen` were initialized there. Napi's manifest is needed even
for an unrelated package because Cargo loads the workspace; tests were not initialized or run.
The first attempt without napi failed with `failed to load manifest for workspace member .../lib/napi`
and `No such file or directory (os error 2)`; after initialization this setup failure disappeared.
Rust **1.96**, installed `wasm32-unknown-unknown` std, pinned Cargo.lock, default debug profile.

Every command below used this prefix (the absolute target directory was exclusively this audit's):

```sh
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=$HOME/wasixv2-orch/logs/browser-design/target
nice -n 19 taskset -c 4-7 cargo check --locked --target wasm32-unknown-unknown -j3 ...
```

| Arguments replacing `...` | Exit | Conclusion |
| --- | --- | --- |
| `-p wasixv2-kernel -p wasixv2-syscalls -p wasixv2-vfs -p wasixv2-vnet -p wasixv2-vfs-mem -p wasixv2-vfs-overlay -p wasixv2-vfs-package -p wasixv2-vfs-host -p wasixv2-vnet-host -p wasixv2-vnet-switch` | 0 | All compile as libraries; abi/pages/wait/host also checked transitively |
| `-p wasixv2-host --no-default-features --features web` | 0 | WebHost compiles |
| `-p wasmer --no-default-features --features js-default,experimental-async` | 0 | JS engine backend and async API compile |
| `-p wasixv2-exec --no-default-features --features js` | 101 | Exec's first Rust error below |
| `-p wasixv2-wasix1 --no-default-features --features js` | 101 | Same exec error; wasix1 itself not reached, not independently established to compile |
| `-p wasixv2 --no-default-features --features js,web,wasix1` | 101 | Same exec error; facade itself not established to compile |
| `-p wasmer-cli --no-default-features --features wasmer/js-default` | 101 | Closest existing JS feature selection, not an advertised CLI browser mode; native dependencies fail in mio |

Exact exec diagnostic, repeated in all three dependent checks:

```text
error[E0425]: cannot find type `Base` in this scope
   --> lib/wasixv2/exec/src/memory.rs:289:60
    |
289 |     (low_guard, read_only, base): (u32, &ReadOnly, Option<&Base>),
    |                                                            ^^^^ not found in this scope
help: consider importing this enum
    |
 30 + use crate::base::Base;
    |
warning: unused import: `RwLockWriteGuard`
  --> lib/wasixv2/exec/src/memory.rs:32:55
```

`base::Base` has an empty non-Linux/non-sys definition, but memory.rs imports the name only
under `cfg(all(unix, feature = "sys"))`. This is a cfg repair, not a reason to invent a browser CoW mechanism.
No temporary source patches were made to look beyond it. Downstream failures may remain.

The CLI's first error is exact:

```text
error: This wasm target is unsupported by mio. If using Tokio, disable the net feature.
  --> .../mio-1.2.3/src/lib.rs:44:1
44 | compile_error!("This wasm target is unsupported by mio. If using Tokio, disable the net feature.");
```

Mio then reports 47 cascades (48 errors total); the exact message/location inventory is in
Appendix A. The CLI unconditionally requests native Tokio runtime/signal and v2 tokio/host-network
features (`lib/cli/Cargo.toml`); `--no-default-features` cannot remove those dependencies.
Do not port the native CLI wholesale. Extract/reuse its engine-neutral configuration and package
assembly at the embedding boundary, leaving terminal OS setup, OS signals, host files, native
cache files and port binding in native glue. A JS SDK uses the facade, not the CLI executable.

Full untouched logs and command records are retained at
`~/wasixv2-orch/logs/browser-design/check-{core,webhost,engine-js,exec-js,wasix1-js,facade-js,cli-js}.log`;
`matrix-status.tsv` records exits. The audit target directory is removed after checks. `cargo fmt --all --check` passes on this
integration audit tree; the older plan branch has the manifest issue recorded in §5.
These are library type checks, not wasm linking, wasm-bindgen processing or browser execution.
Stable std in this probe is not the threaded runtime's rebuilt std.

### Runtime assumptions that a passing check misses

| Location | Assumption / actual browser behavior | Required treatment |
| --- | --- | --- |
| `host/src/web_host.rs` | Date/performance, setTimeout and crypto exist; spawn_blocking invokes work inline; spawn_thread explicitly returns Unsupported | Browser executor and independent event driver; keep simple WebHost for single-event-loop embedding |
| `host/src/web_host.rs:timeout` | Dropped sleeps leave their JS timers alive | Cancellable timer handles, generation check on reply; no early expiry, retain max-delay splitting and random chunking |
| `kernel/src/timer.rs:spawn_driver`, `tty/console.rs:start_pump` | Also use generic ThreadStart, not just guest pthreads | Browser executor must support arbitrary Send start closures and kernel drivers, with lifetime independent of a guest |
| `host/src/tokio_host.rs`, `timer.rs` | Native threads, Condvar, Instant, blocking pool, stdio/CPU clocks | Not enabled in browser feature graph; no fake native syscalls |
| `kernel/src/{futex,sched}.rs` | std available_parallelism falls back to 1 | Not an OS-thread scheduler; expose embedder parallelism through a general host capability only if needed, with approval for trait change |
| `vnet/src/sctp.rs` | std Instant in association creation/data/autoclose paths compiles but is unusable on unknown-unknown | Browser-safe clock inside vnet; prefer injected monotonic source where it must align with kernel timers, otherwise a target-specific clock implementation; retain native behavior |
| `exec/src/cputime.rs` | Native sampler is cfg-disabled on wasm; browser has no native per-thread CPU clock | Existing Host None fallback charges active intervals; do not claim precise CPU-time or real-time scheduling guarantees |
| `vnet-host` | Unix-only mio reactor, host sockets/fds, resolver threads; on wasm these implementations are absent although helpers compile | Separate browser Backend; compiling vnet-host is not networking support |
| `vfs-host` | Linux-only O_PATH/openat2/proc-fd operations; wasm compiles an Unsupported opener | Memory/package/browser provider drivers; never call mount_host_dir for a browser handle |
| `pages`, `exec/{base,pagemap,memory_image,memory}.rs` | Native memfd/mmap/MAP_FIXED/mprotect/pagemap optimizations | Existing full-copy/zero-fill and copied-mapping fallbacks; no raw host address on JS |
| `exec/src/memory.rs:ForeignMemory`, `exec/src/lib.rs` attachment | SharedMemory::host_data returns None on JS; current ForeignMemory thus faults and reports size 0 | Backend-specific shared-memory copies and atomics, including a deliberately specified synchronous foreign-copy path (C1) |
| `syscalls/src/control.rs:SharedMappings::attach_memory`, `kernel/src/mm/retained.rs:sync_copy` | Attached foreign copies must complete synchronously; retained page callbacks use now_or_never and fault Pending | Cannot substitute an async postMessage future without contract approval; see C1 |
| `exec/src/lib.rs:memory_grow` | Unconditionally calls MemoryView::data_ptr before growth; JS data_ptr explicitly panics with `direct data pointer access is not possible in JavaScript` | Restrict native pointer-stability checks to native guarded memories; JS growth uses refreshed buffer views, never data_ptr |
| `exec/src/memory.rs:GuestMemory::compare_exchange_u32` | Real atomic implementation is sys-only; JS inherits read-then-write default | JS Atomics.compareExchange on guest SAB; include alignment, overflow and PageAccess checks |
| `lib/api` JS objects | Rust runtime heap and guest memories are different wasm memories; worker-local JS handles cannot be dereferenced on another worker | Shared-object envelopes, validated IDs, per-worker roots and explicit ownership; raw Rust pointers only refer to the shared runtime heap |
| `exec/{thread,process,fork,asyncify}.rs` | ThreadStart moves engine/module/SharedMemory and shared Rust kernel handles to another executor thread | Browser workers share the runtime wasm heap; each gets its own Store/instance/TLS and transferred guest objects |
| `api/src/lib.rs` | Native defaults include Tokio and host network; host DNS reads /etc/resolv.conf; MachineBuilder always constructs default_network, with no supplied-Net option | Require no-default-features js+web; add general builder injection of an existing Arc<Net>, with explicit browser backend/resolver and byte-backed package/cache |
| `lib/api/src/backend/js/jspi.rs` | Async guest/host calls require WebAssembly.Suspending/promising | Probe capabilities; JSPI lane first, non-JSPI lane required for v1 coverage (C2); guest asyncify for fork is a separate concern |

Ordinary std threads/filesystem/Instant are not valid browser implementations even when Rust
accepts the source. The [Rust target documentation](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)
explicitly describes these standard-library limitations.

## 4. Proposed architecture

### One logical kernel, shared across task workers

Use the kernel's existing Arc-based object graph in **one shared Rust runtime memory**.
Run one guest task per dedicated Web Worker, each with its own instance, Store and Rust TLS.
A separate coordinator/I/O worker owns worker admission, JS-only objects, fetch, relay events,
timer dispatch and UI message routing. Kernel timer/tty futures may run on service workers
through the existing ThreadStart API. The coordinator is not a second kernel and does not
serialize every syscall: task workers invoke the same kernel state directly through Syscalls
and Control. The page's main thread runs JS presentation/message handlers only.

```text
browser UI / SDK API (JS; no kernel locks or Atomics.wait)
        | messages / byte streams
coordinator + I/O worker ---- fetch / websocket relay / filesystem provider
        | admissions, object envelopes, timers, wake messages
        +----------------------+----------------------+
        |                      |                      |
 task worker A            task worker B          service worker(s)
 guest instance A         guest instance B       timers / tty pump / memory copies
        |                      |                      |
        +--- same runtime wasm memory: one Arc<Kernel>, VFS, Net, wait queues ---+

 guest A memory: its own SAB      guest B: same SAB for CLONE_VM, new SAB for fork
```

This preserves synchronous Control methods, wait-queue identity and existing locks. A single
kernel RPC server would have to remote `take_pending`, `work_flag`, shared mappings, task
lifetime and synchronous retained copies as well as every syscall; that is a larger contract
change. Independently instantiating a kernel on each worker would split fd tables, process
trees and futex queues and is incorrect.

The runtime artifact needs imported shared memory and std rebuilt with atomics; guest memory
must **not** be confused with this runtime heap. Match the existing SDK pattern: pinned nightly
with rust-src, `-Z build-std=std,panic_abort`, atomics/bulk-memory/mutable-globals, explicit
shared-memory maximum and wasm-bindgen `--target web`. Fix limits/TLS based on the generated
artifact, not a copied 4 GiB constant. The [wasm-bindgen threading guide](https://wasm-bindgen.github.io/wasm-bindgen/examples/raytrace.html)
explains rebuilding std and independent thread stack/TLS destruction. Do not add nightly or
browser flags to the native workspace defaults.

### Worker/executor protocol and forward progress

Keep `Host`, `Executor`, `ThreadStart`, `ByteSource`, `ByteSink` and `Console` as the public
host boundary. Add a browser executor next to simple WebHost, with async construction so its
coordinator and service resources are ready before Machine boot. It implements:

- `spawn_local`: poll a non-Send future only on its owning worker. Its Waker publishes a
  worker/task ID and an atomic wake generation to a shared ready queue, then notifies the
  worker. Never invoke a worker-local wasm-bindgen future queue from a foreign worker.
- `spawn_thread`: transfer ownership of one boxed Send ThreadStart in the trusted runtime
  heap, with the existing Wasmer shared-object envelope for modules and memories. Claim it
  exactly once after worker bootstrap/TLS/object imports. The worker drives start() locally;
  the future itself never crosses workers. This supports guest tasks and timer/tty drivers.
- `spawn_blocking`: run the Send closure on a dedicated work worker, report completion/panic
  through a shared completion slot; the waiter stays async. Do not execute it on UI or the
  network/timer event loop. Panic-abort of the runtime is infrastructure failure, not a
  successfully resumed native Rust panic.
- Admission must reconcile the existing synchronous io::Result with asynchronous worker
  bootstrap: prewarm resources; if a new worker is needed, an acknowledged bootstrap can
  park only the calling worker while the independent coordinator creates it. Publish
  acceptance only when ownership transfer can succeed. Canceled or failed admissions drop
  an unclaimed closure once and return a resource error; exec's existing abort path removes
  the child and reports EAGAIN. No accepted task may silently disappear on worker startup.

Use atomic sequence predicates around every wait, with release/acquire publication and a
recheck after registering. Spurious wakeups, completion-before-wait and buffer reuse must
be harmless. Bound queue and byte-buffer growth and return backpressure/resource failure.
Do not cap execution to a fixed number of occupied workers: all of them might wait for a
new runnable child. Grow to a configured admission limit; fail promptly when it is reached.
Service workers have reserved capacity and never queue behind guest work they must unblock.

In JSPI mode a pending syscall returns to the worker event loop; task dispatch uses messages
or another task-level yield so a compute loop's preemption hook also lets browser events run.
A chain of resolved Promise microtasks alone can starve incoming messages. The existing
`exec/src/lib.rs:yield_now` polls Pending once and self-wakes; the browser executor must
turn this into a fair event-loop task yield under sustained preemption, not an endless
microtask chain. Keep the existing
preemption instrumenter and return-to-guest hooks. In non-JSPI mode (C2), a task worker may
park in Atomics.wait, but **all** promise-producing I/O and timers must then live elsewhere.
[Atomics.wait](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Atomics/wait)
blocks its agent, and cannot be used on the browser main thread.

No synchronous kernel locks are held across JS event-loop waits or admission handshakes.
Existing async VFS locks may span provider calls under their ordinary lock order; the provider
must not reenter that locked operation. Audit synchronous critical sections before sharing the
runtime across workers. A worker's
local JS objects remain on that worker; Send Rust wrappers carry IDs/shared ownership only.
On normal shutdown: signal group exit, wake blocked tasks, run exit_mm/robust cleanup and
release, drain outputs, drop references, collect JS registry objects, destroy thread TLS and
stack, then terminate workers. Abrupt worker death can leave shared Rust locks/allocator state
unusable: fail the affected Machine/runtime coherently instead of reusing its heap blindly.

### Host clocks, timers, entropy and signals

`realtime` uses Date; monotonic uses one normalized clock origin for the whole Machine.
`performance.now()` alone has a worker-specific origin: normalize via timeOrigin or a shared
boot epoch and clamp reads to a common nondecreasing timeline. Kernel Timekeeper continues
to implement guest clock_settime, NTP and time namespaces; never change the browser's clock.
Return None for unavailable native thread CPU clocks and document active-wall-time accounting.

Move deadline ownership to a cancellable service-worker timer queue. Preserve
`Host::sleep_until_range`'s soft deadline/slack semantics; no early completion, recheck the
clock, split delays above the JS timer limit, and cancel dropped waits using generations.
Background-tab throttling can delay observation, as host scheduling can; it must not cause
busy polling or an early success. The kernel's timer state and signal generation remain
unchanged. Linux authority: `kernel/time/hrtimer.c:hrtimer_nanosleep`,
`__hrtimer_run_queues`, and `include/linux/hrtimer.h:hrtimer_set_expires_range_ns`.
Entropy remains crypto.getRandomValues in at most 65536-byte chunks.

Signals remain kernel pending state and work flags, delivered by the target task through
existing syscall-return and preemption paths. Route UI kill/resize/hangup to Signaller on a
worker; raw Ctrl-C bytes go through n_tty. Wake the specific blocked task's scheduler, not
all workers and not a fabricated user signal. Untimed/timed atomic-wait restart behavior,
SA_RESTART, stop/continue and exec/exit cleanup stay existing kernel behavior. Linux authority:
`kernel/signal.c:{complete_signal,signal_wake_up_state,get_signal}`,
`kernel/exit.c:{do_exit,exit_mm}`, `kernel/fork.c:mm_release`.

### UserMemory, foreign copies and futexes

Keep UserMemory/PageAccess and kernel error precedence. Never eagerly decode all guest buffers
into a JS RPC request: `read(-1, NULL, n)` still needs EBADF before EFAULT. Local GuestMemory
uses JS backend typed-array copies with checked u64 arithmetic and refreshed memory.buffer
views after growth; a guest SAB cannot become a Rust raw pointer in the runtime's memory.
In particular, remove the unconditional data_ptr call from the JS memory_grow hook; its
native guard-stability assertion is meaningful only for native guarded memories.
Implement JS Atomics.compareExchange on a checked aligned Int32Array view of **guest** memory;
use the returned bit pattern as u32. The read-then-write default is wrong with concurrent guests.

Keep wasm memory.atomic.wait/notify rewritten to Control::AtomicWait on kernel futex queues.
Use Atomics wait/notify for the **runtime scheduler's** parking words, not as a parallel
implementation of Linux futex state. This preserves private/shared keys, PI, signal restart,
timeouts, clear_child_tid and robust-owner death. Linux authority:
`kernel/futex/core.c:{get_futex_key,handle_futex_death}` and
`kernel/futex/waitwake.c:{futex_wait_setup,futex_wake}`.

**C1 — approval required before implementation: browser foreign-memory publication and
synchronous access.** `SharedMappings::attach_memory` currently promises copies on the
calling thread, synchronously, including in retained-page timer/wake callbacks. JS has no
host_data pointer and only worker-local JS references. Keep that immediate-result UserMemory
shape; propose explicitly allowing a browser foreign handle to synchronously use an
independent memory-copy worker when the caller lacks a local reference. This relaxes the
“on the calling host thread / never block a guest executor” execution assumption; it must
not be smuggled in as an ordinary async UserMemory implementation.

The proposed protocol is concrete:

1. Exec registers a newly instantiated shared guest Memory with the browser memory service
   and awaits an acknowledgement **before** attaching/exposing its foreign handle or starting
   the task. Add an execution-side publication interface; Host stays engine-independent.
   Existing native execution uses an immediate no-op. This adds an exec readiness contract,
   although no guest syscall signature changes.
2. ForeignMemory stores a shared-memory ID and generation, not a native pointer. If already
   delivered locally, copies can use local JS views. Otherwise its synchronous copy uses a
   bounded SAB request/reply slot and Atomics wait/notify on the calling worker. The memory
   service owns a rooted JS Memory and copies only bytes; it never enters the kernel, guest,
   Store, mm lock or retained-page locks. The caller validates PageAccess and protections
   under the existing mm operation/retention serialization, then holds their required leases
   until the byte copy completes. The service cannot wait for those locks or for the caller.
3. Bounds/size are read from the current JS Memory view on every operation; growth refreshes
   the service's view. No Rust slices alias the guest. Compare/exchange uses JS Atomics.
   Empty, end-of-memory, overflow, forced access and protection behavior match local copies.
4. IDs retain the memory through exit, registration and in-flight requests; generations
   reject stale handles after exec. Reply publication is the linearization of transport
   completion, not an atomic snapshot of the guest's racing bytes. Cancel/teardown cannot
   recycle a reply buffer while its writer can still write it. A dead memory service is a
   Machine infrastructure failure; never silently turn every pointer into EFAULT.

This preserves synchronous completion needed by `kernel/src/mm/retained.rs:sync_copy` and
foreign access to a running **or blocked** target, without requiring target-worker message
handling. It is a controlled worker-blocking data path with an independent service, not a
kernel RPC server. A pure async foreign-memory design instead requires redesigning retained
page callbacks and all their lock/cancellation assumptions; that alternative is larger and
also requires approval. If C1 is rejected, stop its dependent work and report the missing
remote-memory capability; do not claim browser parity by hiding it behind blanket errors.
Linux authority: `mm/memory.c:__access_remote_vm`, `mm/process_vm_access.c:process_vm_rw_core`,
`fs/proc/base.c:mem_rw`, `io_uring/rsrc.c` retained-resource lifetime.

### Fork, clone, vfork and exec

Reuse the existing execution state machine. CLONE_VM creates another guest instance on the
same guest SAB, with separate stack/TLS and the same kernel Mm; CLONE_THREAD controls the
thread group independently. Start through the existing ABI thread entry. Do not structured-clone
an Instance or Store. Deliver shared Module/Memory objects before executing its ThreadStart.

Fork remains the asyncify configuration only: unwind, capture linear bytes and mutable globals,
create a new memory, restore, bind the child, and rewind both sides. The non-native full-copy
MemoryImage path already exists; bound copy scratch and report allocation failure. No native
pagemap, mmap, sparse CoW claim, or alias of the parent's whole memory. Preserve wipe/dontcopy
ranges and late parent/child TID writes. All bootstrap failures must release the created task.

Vfork still changes the exec task pointer on the same instance until child exec/exit, uses the
same Mm, resumes through the existing guest export, and retains killable-parent/de_thread
ordering. Exec on the current task replaces its image; a detached vfork child or leader
handoff starts through the browser executor. Publish the new foreign handle (C1), do not
leave the old handle attached to the new Mm. Preserve exit_mm/robust release before wait
reports death. Linux authority: `kernel/fork.c:{copy_process,wait_for_vfork_done,complete_vfork_done}`,
`fs/exec.c:{de_thread,begin_new_exec}` and `kernel/exit.c:do_exit`.

**C2 — approval required for the non-JSPI execution lane.** JSPI is the first supported lane
because the JS backend already implements it. Runtime probing must check Suspending/promising,
shared memory/atomics and the guest's required Wasm features, including modern EH/exnref when
needed. The [engine's JSPI documentation](https://v8.dev/blog/jspi) explains its Promise suspension
boundary. Do not equate JSPI availability with guest asyncify exports.

V1 parity cannot be declared by requiring JSPI everywhere v1 previously worked. The proposed
fallback is a JS-backend async-host adapter that drives a syscall future synchronously on a
**task worker**, parks it with the shared scheduler on Pending, and resumes it on a cross-worker
wake. The existing public async-call API can retain its shape, but this changes exec's suspension
and event-loop progress contract and needs approval. Route every awaited JS promise to the I/O
worker, including loader fetch, filesystem providers and timers. Nested signal/vfork calls,
exceptions crossing host frames and asyncify unwind/rewind must have explicit tests; never
attempt to asyncify EH code indiscriminately. If this adapter cannot preserve these invariants,
report the unsupported browser/guest matrix and seek an execution-contract decision rather
than shipping a partially working fallback. JSPI-only delivery is an intermediate milestone.

### Networking behind vnet's existing traits

Implement `wasixv2-vnet-web` as a sibling of vnet-host, implementing Backend and nonblocking
BackendStream/BackendDatagram. This is a new platform driver, not another VFS or socket stack.
Browser JS websocket/fetch objects live in its I/O worker; shared queues contain payloads,
state and readiness. Operations return EAGAIN/EINPROGRESS immediately; completion updates
queues and wakes the socket's **existing** WaitQueue, outside backend locks.

Retain v2's local ports, routes, namespaces, listeners, UDP, Unix sockets and vnet-switch.
The external relay connects an already-decided destination. It must not reimplement guest
bind conflicts or create a host socket on guest bind. The minimum transport is the classic
v1 relay request/reply protocol from virtual-net client/meta/server, translated in the driver;
implement only the compatible wire records/codec needed by this driver, privately or in an
existing engine-neutral protocol location. Keep DESIGN's dependency rule: no v2 runtime
dependency on lib/wasix, virtual-net, virtual-fs or virtual-io. Test protocol compatibility
against the reference relay without importing its runtime abstractions into v2. WISP is an additional transport via SDK/provider callbacks, with
explicit capabilities rather than pretending to speak the same protocol.

Required semantics: bounded stream buffers, partial writes, per-flow ordering and backpressure,
UDP message boundaries/source addresses, IPv4/IPv6, half-close/reset/EOF, connection errors and
SO_ERROR, cancellation, and last-reference close. Reconnect may create **new** connections;
it cannot replay bytes into an old TCP stream. Map explicit relay errno records; websocket
transport failures do not magically identify a Linux peer error. Unsupported transport options
must return the appropriate existing backend error rather than falsely claim wire effects.
Classic TCP/UDP relay support is mandatory; current WISP TCP/DNS cannot replace the UDP lane.

Backend::lookup serves v2's existing 10.0.2.3 DNS server, including A/AAAA/CNAME, PTR and raw
forward requests. Use relay resolution or configured DoH via fetch with explicit CORS policy;
never read the browser host's /etc/resolv.conf. A failed resolver completes promptly and wakes
all waiters. DNS over the guest socket API stays visible to syscall tracing/policy.

For ingress, implement publication through a relay that supports it, or a browser stream source
feeding InboundSink. Preserve poll_room/backlog, explicit publication handle lifetime and v2's
rule that publication failure is an embedder error, not a guest bind failure. SDK service-worker
HTTP builds on this byte stream path. No browser listens on a native host TCP port. Read-only
listener discovery for SDK ports belongs in the embedding facade, not in guest ABI.
Linux authority: `net/ipv4/tcp.c:{tcp_poll,tcp_close,tcp_shutdown}`,
`net/ipv4/udp.c:udp_recvmsg`
and `fs/eventpoll.c:ep_poll` for readiness observations.

Fetch is not a transparent replacement for guest TCP/TLS: CORS, browser cookies, forbidden
headers and streaming behavior are observable. Keep registry/download fetch separate from
socket relay. The missing v1 HTTP-client import is a compat provider in wasix1, with its
existing v1 ABI layouts and behavior; it must not become a Linux kernel syscall.

### Filesystems and terminal

Use the existing memfs/overlay/package drivers for writable roots, WEBC volumes, command files,
/tmp and shared directory instances. Host-provided browser storage implements the existing
async Filesystem/Node/Handle interfaces, next to platform drivers; JS references stay on the
provider's event loop, and requests carry inode/object IDs and byte names. VFS owns traversal,
permissions, mount rules and Linux errors. Drivers receive one component, never a host path
that bypasses the VFS. Stable inode identities and generation/cookie behavior must survive
rename/unlink while open. Provider capability limits are explicit; browser File System Access
is not a native Linux inode filesystem. An initial in-memory mount created from JS files is
full parity with classic Directory; persistent OPFS/File System Access are optional providers.
Linux authority: `fs/namei.c:{link_path_walk,do_open}`, `fs/open.c:do_sys_openat2`,
`fs/read_write.c:vfs_read/vfs_write` and `fs/readdir.c:iterate_dir`.

Streams implement ByteSource readiness and async ByteSink with bounded queues. Distinguish
plain process stdio EOF from terminal ^D and hangup. Use kernel pipes if Linux pipe capacity,
PIPE_BUF, poll HUP and SIGPIPE are required; do not label a generic callback sink a kernel pipe.
The kernel's console input pump runs independently of a busy guest, so Ctrl-C takes effect
without read(). JS terminal widgets only encode keys/show bytes; termios, echo and job control
stay v2. Winsize calls console_resize; closing the terminal calls console_hangup. Support
piped stderr independently and drain all streams on exit. Linux authority:
`drivers/tty/n_tty.c:{n_tty_receive_buf_common,n_tty_read}`,
`drivers/tty/tty_io.c:do_tty_hangup`, `fs/pipe.c:{anon_pipe_read,anon_pipe_write}`.

### Limits that must remain explicit

SAB and cross-origin isolation are a deployment requirement for the full threaded profile;
[SharedArrayBuffer documentation](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer)
describes the shared data and agent-cluster restrictions. Check secure context, isolation,
worker asset/CSP loading and Wasm capabilities before accepting a run; do not hang later.
A deliberately restricted nonshared profile may be offered, but cannot be labeled full parity.

Browser Wasm cannot alias part of one guest linear memory into another, protect subpages or
supply a native fd/mmap. Keep DESIGN's explicit copied-file/SysV mapping synchronization and
no native shared-page capability. Threads on the same SAB are coherent; independently forked
memories do not gain coherent MAP_SHARED anonymous pages just by using SAB for both. Existing
AIO/io_uring mapped rings requiring SharedPages remain unavailable under those paths; remote
registered-page copies must still work wherever the existing interfaces allow them. Do not
claim Linux-exact interprocess shared mappings, read-only guest page faults or native CoW in
browser. Achieving those would require a separately approved guest memory transformation,
outside this plan. Keep these platform limits separate from ordinary browser parity failures.

## 5. Validation and gates to implement

No test suite was run for this design. All tests below are future acceptance requirements.
Every implementation change gets a regression unit test that fails before it; add guest
programs only in wasixv2-tests and build from source. Never commit generated wasm. Keep all
v1-specific expectations and HTTP import tests in wasix1. Use existing siblings:
`lib/api/tests/jspi_async_js.rs`, JS memory tests in lib/api,
`lib/wasixv2/host/src` unit tests, `kernel/src/tests/{futex,signal,tty,...}.rs`,
`exec/src` WAT tests, and `tests/wasixv2/suite/<name>/main.c` plus its fixture metadata.
A browser integration harness in v2 is new: there is no existing v2 headless-browser runner.
Place it with the exec/facade integration tests, following the JS engine test style, and
extend the separate suite runner rather than copying programs into this repo.

### Compile and link gates

1. Add a required lightweight wasm32 library-check job to `.github/workflows/test.yaml`,
   separate from native lint and the existing Node-only test-js job. Reproduce every passing
   core row and all js/web exec/wasix1/facade rows from §3; the latter must become green.
   Keep host-only vnet/vfs rows as portability checks, not capability assertions. Check each
   feature combination independently so native/default feature unification cannot mask errors.
2. Add a required **threaded debug link** of a minimal browser embedding harness, rebuilt std,
   pinned nightly/rust-src and matching wasm-bindgen CLI/lockfile version. Process with bindgen,
   inspect imported shared runtime memory, maximum, TLS exports and wasm feature set. This
   catches what cargo check misses. Do not turn on release builds for this gate.
3. Save `cargo tree -e features` evidence: no tokio native runtime/net/signal, mio reactor,
   native compiler backend or host getrandom backend in the browser runtime. Keep native
   compiler dependencies out of runtime artifacts; report excluded-team dependencies rather
   than editing their mechanisms. Native CLI checks stay native; a browser facade smoke must
   assemble the same packages/configuration without CLI OS dependencies.
4. Touched crates get their own native tests, clippy `--all-targets --locked -- -D clippy::all`
   with Makefile `lint-packages` RUSTFLAGS, plus wasm library clippy and `cargo fmt --all --check`.
   Host-only test dev-dependencies may need target gating to permit browser tests; do not enable
   cranelift or Tokio multi-thread via wasm dev-feature unification. Excluded crates are owned
   by their team. On this machine: CARGO_INCREMENTAL=0, nice 19, taskset 4-7, -j3, one heavy
   command at a time, no release builds and no broad suite runs without authorization.

The plan branch itself currently cannot run workspace cargo fmt: its `lib/wasixv2/*` member
glob includes the existing documentation-only `lib/wasixv2/plan` directory and fails because
`plan/Cargo.toml` does not exist. Do not add a fake crate to repair a documentation gate.
Implementation gates run on the integration source tree. No Rust crate is changed by this plan,
so crate tests/clippy are not applicable to this documentation commit.

### Headless browser gate

Use wasm-bindgen-test for memory/atomics/async-host unit behavior and Playwright for an actual
isolated page, worker creation, SDK-shaped streams and multiple-runtime lifecycle. Serve
local assets and fixtures with COOP same-origin and COEP require-corp (plus appropriate CORS/CORP
for fixture requests). Pin browser versions and log capabilities rather than infer them from
user-agent names. Start with Chromium JSPI; add Firefox/WebKit and forced-no-JSPI coverage for
C2. Missing capabilities in a required lane fail, not skip all tests and report green. Browser
coverage advertised to users is exactly the passing matrix, not a blanket “all browsers”.

Exercise: two tasks on one guest SAB and one kernel; compare/exchange under contention;
notify-before-wait; canceled timers; nested async calls/EH; busy-loop kill; futex wait interrupted
by signal; thread join/clear_child_tid; fork copy and vfork/exec handoff; remote memory while
the target blocks or grows; retained memory after unmap/exec; worker bootstrap failure;
service-worker starvation; output backpressure/drain; tty Ctrl-C/resize/hangup; reused worker
TLS and shared-object collection. A heartbeat on the UI and I/O worker detects event starvation.
Normal close must leave no live task workers, deadlines, relay streams or retained guest memory.

Use a deterministic local relay fixture for TCP echo, half-close/reset and UDP, plus DNS and a
local fetch/CORS server. Include disconnect while blocked, transport refusal, no provider,
queue saturation, IPv6 where supported, endpoint replacement and reconnect without byte replay.
Test backend behavior independently of any public internet service or registry availability.

### Browser subset of wasixv2-tests

Add a build-only/export mode and browser-runner adapter to the existing native fixture builder
in the **wasixv2-tests repo**: emit selected guest wasm, args/env, mount files, expected exit/output
and timeouts as a manifest; then serve it to the same browser harness. Reuse fixture discovery,
compiler configuration (including asyncify for fork) and expectation parsing. The runner itself
is native tooling; it does not compile to browser. Unknown metadata fails explicitly.

Initial allowlist candidates already present at the pinned test tree:
`base-args-env`, `base-exit-status`, `base-getrandom`, `base-clocks`, `base-nanosleep`,
`base-timerfd`, `fs-rw-pread`, `fs-readdir`, `fs-rename`, `fs-link-symlink`,
`fs-openat2-in-root`, `mmap-brk-layout`, `mmap-file-private`, `mmap-file-shared-msync`.
Next include actual `thr-*`, `sig-*`, `proc-*`, tty and loopback networking fixtures after
reviewing their metadata for host dependencies; use the existing
`thr-atomic-wait-signal` and fork/exec/vfork fixtures rather than weaker duplicate examples.
Add required browser cases missing from those sources as ordinary source fixtures.

Native `fs-mapped-dir` needs a provider-backed test mount, not a host path in the browser.
Tests requiring native port publication, OS /proc, mprotect traps or coherent separately mapped
host shared pages get explicit capability annotations and reasons; do not bless them as
browser expected failures to hide implementation bugs. Maintain a reviewed browser allowlist
and platform-limit list without editing native expected-failure baselines. Required threading,
fork in asyncify, signals, relay TCP/UDP, mounts and TTY cases cannot be quietly skipped.

## 6. Work packages and dependency order

Each package is an independent implementation branch from the integration head, one owner,
regression tests in the appropriate crate/repo, conventional commits and the exact attribution
required by the owner. The table identifies file ownership to minimize conflicts. “New” paths
are proposed siblings, not existing files. Avoid editing excluded mechanisms. Dependencies
on C1/C2 are approval gates; this document does not approve their implementation.

| WP | Scope and owned files | Dependencies | Acceptance tests / completion |
| --- | --- | --- | --- |
| B0: contract decisions | Record approval or rejection of C1 foreign-copy/publication and C2 non-JSPI suspension. Document worker admission failure semantics, supported browser capability matrix, and unavailable native shared-page features in DESIGN after agreement. No implementation before these decisions. | This plan | Reviewed protocol invariants, explicit actor/lock ownership and failure ordering; each dependent WP knows whether it is allowed to change its contract |
| B1: restore wasm build gate | `exec/src/memory.rs` cfg/import repair; target-gate native-only test dependencies in exec/wasix1 as necessary; `.github/workflows/test.yaml` compile-only rows, Makefile check target. Do not hide uncompiled modules with broad cfgs. | None; can start while B0 is reviewed | Exact §3 core/WebHost/engine/exec/wasix1/facade checks green; regression compile gate fails at baseline; native exec unit checks, clippy and fmt. Report newly exposed failures including excluded-team dependencies |
| B2: shared runtime and worker executor | `host/src` browser executor/JS bootstrap siblings, `host/Cargo.toml`, browser harness build recipe. Own admission, ThreadStart/closure ownership, TLS, object envelope, cross-worker Waker and worker shutdown. Keep engine-dependent object plumbing in exec/API; host sees generic envelope callbacks. | B1; admission design from B0 | Linked threaded debug harness; two workers share a Rust Arc and guest SAB; kernel timer/tty closures work; wake-before-wait, worker creation error/limit, no leaked closures/TLS, idle worker reuse, UI never blocks |
| B3: JS user/foreign memory | `exec/src/memory.rs` JS implementation, `exec/src/lib.rs` publication call, narrow `lib/api` shared-memory copy/atomic helpers, exec-side publisher interface; coordinate B1's small import fix first. No kernel algorithm changes. | B1, B2, **C1 approval** | Atomic CAS stress with guest workers; JS memory_grow without data_ptr panic, EFAULT/protection/bounds/growth; foreign read/write while target runs or sleeps; immediate completion in retained callbacks; old-mm memory retained across exec/unmap; failed publication creates no observable runnable task |
| B4: clocks, timers and progress | `host/src/web_host.rs` and timer driver siblings, scheduler wake integration; `vnet/src/sctp.rs` browser-safe clock. Preserve native clock paths. Coordinate host module exports with B2 owner. | B2; can parallelize with B3 | No-early sleep, slack/deadline, long timeout splitting, cancel/drop, common worker clock epoch, crypto chunks; timer/tty service progresses under busy guests; SCTP association/autoclose no std Instant panic; fake-clock native regression plus real browser checks |
| B5: socket relay backend | New `vnet-web` crate beside vnet-host; async JS I/O worker transport, bounded shared buffers and relay codec adapter. Own backend manifests only; facade wiring deferred to B8. | B1; backend unit model can start immediately, worker integration after B2 | vnet Backend conformance, local relay TCP/UDP/DNS and IPv6 capability; partial writes/readiness, half-close/reset, cancellation, disconnect, reconnect-no-replay, publication backlog; WISP provider transport with explicit TCP/DNS capability |
| B6: filesystem providers | New browser-provider driver sibling of vfs-host or a small existing-trait adapter if sufficient; vfs-mem/package/overlay behavior reused. Own provider inode/handle protocol and optional persistence. | B1; independent of B3/B5, worker wiring after B2 | Shared JS-created mounted directories; guest and embedder see writes/rename/unlink/open handles; VFS openat2 containment, permissions, readdir cookies, readonly errors and concurrent append. In-memory mounts mandatory; optional OPFS does not block classic parity |
| B7: stdio and TTY adapters | Browser ByteSource/ByteSink queue adapters and harness stream endpoints; reuse `kernel/src/tty` without policy changes; narrow facade signaller/stdio additions only if existing APIs cannot express parity. | B2; may parallelize with B3-B6 | Stream EOF/cancel/backpressure, independent stderr, exit drain; canonical/raw tty, Ctrl-C busy guest, resize/SIGWINCH, foreground job control and hangup; no duplicate line discipline in JS |
| B8: embedding/package glue | `api/src/lib.rs`, `api/Cargo.toml`, engine-neutral CLI assembly boundary as necessary, browser harness crate facade; wire backend/provider/host; add general supplied-Net builder injection, select loader and explicit features. No broad CLI wasm port. | B1, B2; finish after B3-B7 | WEBC/raw/module and command runs, args/env/cwd, injected packages, shared directory mounts, two Machines isolated, shutdown/kill futures resolve; no native Tokio/mio/OS defaults in browser feature graph; byte-backed registry/cache integration works |
| B9: v1 compatibility gaps | `wasix1/src` only for v1 HTTP client provider/import layouts, defaults and confirmed browser ABI quirks; reuse v2 syscall/control path. SDK package resolver/fetch stays outside kernel. | B3, B8; inspect layouts before code | v1 static program/threads/spawn/fork/tty/socket tests and HTTP fetch mock including CORS/failure; existing v1 preopens and command defaults; no v1 branches in kernel; validate missing imports explicitly |
| B10: non-JSPI parity | `lib/api/src/backend/js` async host-call adapter, `exec` suspension integration only as approved, with B2/B4 driver operations available off-worker. Do not alter guest ABI or excluded functionality. | B2-B4, **C2 approval**; B8 for full end-to-end | JSPI deliberately absent: blocking read/sleep/futex, busy signals, nested handlers/EH propagation, asyncify fork/vfork/exec and provider/relay I/O pass with live UI/driver; exact supported browser/guest matrix documented |
| B11: browser harness and suite adapter | `lib/api` and exec/facade JS integration tests, CI headless job; **wasixv2-tests** runner/export mode and source fixtures on a separate branch; no expected-failure blessing. Skeleton can start with B1. | B1 skeleton; B2-B9 for JSPI matrix, B10 for full parity | Required compile/link/browser lanes and reviewed fixture subset pass; fault/lifetime/stress cases from §5; deterministic relay/fetch/provider services; CI artifacts include capabilities, logs and per-test result; zero silent skips of parity features |
| B12: SDK handoff and parity signoff | Wasmer browser embedding example/docs and capability contract; separate SDK port follow-up owns JS public wrappers, worker URLs/bundling, registry auth/cache/progress, service-worker HTTP and API compatibility tests. | B8-B11 | Run the classic and current capability checklist in §2 against ported SDK; feature matrix and performance/resource baselines published; all mandatory browser lanes green before declaring full support |

Suggested parallel waves (independent branches, serialized local builds on this machine):

1. B0 review; B1 compile fixes/gate; B5 transport model; B6 provider model; B11 harness skeleton.
2. B2 runtime/executor, then B3 memory; B4 timer/progress, B5 I/O integration, B6 providers,
   B7 streams/tty can proceed with assigned host-module ownership.
3. B8 facade assembly and B9 compat; B10 approved fallback; B11 browser fixture execution.
4. B12 SDK port handoff/signoff. A JSPI-only milestone may precede B10, clearly labeled.

Do not merge all worker/memory changes as one unreviewable package: land the admission and
ownership protocol, its failure tests, then local atomics, then foreign publication/copies.
No test completion is implied by native feature-completeness. Browser parity remains open
until the table's required capabilities and their failure/lifecycle tests pass.

## Appendix A. Exact CLI diagnostic inventory

The first compile_error and every subsequent error header are recorded below with the
reported dependency source location. Full multi-line diagnostics remain in check-cli-js.log.
Paths are relative to the cached `mio-1.2.3/src/`; error ordering is compiler output order.

```text
lib.rs:44:1: error: This wasm target is unsupported by mio. If using Tokio, disable the net feature.
io_source.rs:14:5: error[E0432]: unresolved import `crate::sys::IoSourceState`
net/tcp/listener.rs:23:17: error[E0432]: unresolved import `crate::sys::tcp`
net/tcp/stream.rs:17:17: error[E0432]: unresolved import `crate::sys::tcp`
poll.rs:322:18: error[E0433]: cannot find `Selector` in `sys`
event/event.rs:24:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:38:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:43:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:68:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:99:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:129:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:151:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:173:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:183:14: error[E0433]: cannot find `event` in `sys`
event/event.rs:221:26: error[E0433]: cannot find `event` in `sys`
net/tcp/listener.rs:120:18: error[E0433]: cannot find `tcp` in `sys`
net/udp.rs:126:14: error[E0433]: cannot find `udp` in `sys`
net/udp.rs:555:14: error[E0433]: cannot find `udp` in `sys`
poll.rs:277:20: error[E0425]: cannot find type `Selector` in module `sys`
poll.rs:736:44: error[E0425]: cannot find type `Selector` in module `sys`
waker.rs:79:17: error[E0425]: cannot find type `Waker` in module `sys`
waker.rs:87:14: error[E0433]: cannot find `Waker` in `sys`
event/event.rs:18:17: error[E0425]: cannot find type `Event` in module `sys`
event/event.rs:187:55: error[E0425]: cannot find type `Event` in module `sys`
event/event.rs:191:41: error[E0425]: cannot find type `Event` in module `sys`
event/event.rs:217:46: error[E0425]: cannot find type `Event` in module `sys`
event/events.rs:43:17: error[E0425]: cannot find type `Events` in module `sys`
event/events.rs:94:25: error[E0433]: cannot find `Events` in `sys`
event/events.rs:189:47: error[E0425]: cannot find type `Events` in module `sys`
net/tcp/listener.rs:82:24: error[E0425]: cannot find value `listener` in this scope
net/tcp/listener.rs:84:15: error[E0425]: cannot find value `listener` in this scope
net/tcp/listener.rs:94:17: error[E0425]: cannot find value `listener` in this scope
net/tcp/listener.rs:95:12: error[E0425]: cannot find value `listener` in this scope
net/tcp/stream.rs:97:18: error[E0425]: cannot find value `stream` in this scope
net/tcp/stream.rs:98:12: error[E0425]: cannot find value `stream` in this scope
net/tcp/listener.rs:82:9: error[E0425]: cannot find function `set_reuseaddr` in this scope
net/tcp/listener.rs:163:20: error[E0599]: no method named `register` found for struct `IoSource<std::net::TcpListener>` in the current scope
net/tcp/listener.rs:172:20: error[E0599]: no method named `reregister` found for struct `IoSource<std::net::TcpListener>` in the current scope
net/tcp/listener.rs:176:20: error[E0599]: no method named `deregister` found for struct `IoSource<std::net::TcpListener>` in the current scope
net/tcp/listener.rs:299:9: error[E0308]: mismatched types
net/tcp/stream.rs:334:20: error[E0599]: no method named `register` found for struct `IoSource<std::net::TcpStream>` in the current scope
net/tcp/stream.rs:343:20: error[E0599]: no method named `reregister` found for struct `IoSource<std::net::TcpStream>` in the current scope
net/tcp/stream.rs:347:20: error[E0599]: no method named `deregister` found for struct `IoSource<std::net::TcpStream>` in the current scope
net/tcp/stream.rs:470:9: error[E0308]: mismatched types
net/udp.rs:633:20: error[E0599]: no method named `register` found for struct `IoSource<std::net::UdpSocket>` in the current scope
net/udp.rs:642:20: error[E0599]: no method named `reregister` found for struct `IoSource<std::net::UdpSocket>` in the current scope
net/udp.rs:646:20: error[E0599]: no method named `deregister` found for struct `IoSource<std::net::UdpSocket>` in the current scope
net/udp.rs:769:9: error[E0308]: mismatched types
```

## 7. Decisions (orchestrator, 2026-10-30, after independent review)

**C2: flipped.** The synchronous-blocking worker lane is the **primary** browser lane: each guest task
runs on its own dedicated worker; async host imports are exposed as synchronous JS functions that poll
the syscall future and park the worker on the shared scheduler (Atomics.wait) on Pending, as v1 did
(`lib/virtual-io/src/waker.rs`, `lib/wasix/src/syscalls/mod.rs:block_on_with_timeout`; classic SDK
`src/tasks/thread_pool.rs`). `call_async` becomes a direct synchronous call on that lane. JSPI is an
optional, runtime-probed optimisation, not a requirement. Consequences:
- No exec code may await a JS Promise on its own task worker (instantiation, loader fetch, providers,
  timers go to the coordinator/I/O worker).
- Failing to start a worker for kernel drivers (timer, tty) is a hard error, never a fallback onto the
  current (blocking) worker (`kernel/src/timer.rs` fallback must not apply in the browser).
- The coordinator creates all workers; a blocked task worker must never be a worker's creator.
- Do not hold the store write lock across nested re-entrant calls (signals, vfork, fork rewind).
- The first required browser CI lane is "SharedArrayBuffer + cross-origin isolation, no JSPI".
- JSPI-specific fairness/yield machinery from §4 is deferred.

**C1: approved with changes.**
- Fast path: local copy through the worker's own registered copy of the shared Memory (existing
  lib/api per-worker handles). Slow path: a dedicated memory-copy worker.
- The memory-copy worker is **pure JS and never executes runtime Rust code** (hard rule; otherwise
  runtime mutexes/allocator create a deadlock cycle).
- The runtime heap is shared, so the copy worker copies directly between guest Memory and the caller's
  Rust buffer; one request slot per task worker in the runtime heap; no bounce buffer.
- No registration acknowledgement: exec posts the registration and attaches immediately; the copy worker
  holds requests for not-yet-registered IDs until the registration arrives.
- Never-reused u64 IDs instead of generations; unregister on Drop.
- Timed Atomics.wait; on timeout the Machine fails (never a silent EFAULT). No kernel path reaches it on
  the page main thread.

**Re-ordering.** B3 splits into **B3a** (JS compare-exchange on guest memory, memory_grow without
`data_ptr`; no gate, right after B1) and **B3b** (foreign copies per C1). **B10 (sync lane) moves into
wave 2 alongside B2** and is the primary execution lane; B4's scheduler wake integration targets it.
