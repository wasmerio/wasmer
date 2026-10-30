# Threaded browser executor

This is the B2 host runtime, separate from the single-event-loop `WebHost`.
The primary lane uses synchronous dedicated task workers and `Atomics.wait`;
JSPI is not required. The smoke harness deliberately removes JSPI in every worker.

## Build and run

Prerequisites: `nightly-2026-08-08` with `rust-src`, the stable workspace toolchain,
Node 22 or later, Chromium, and a `wasm-bindgen` CLI matching `Cargo.lock` (0.2.129).
Install the nightly without changing defaults:

```sh
rustup toolchain install nightly-2026-08-08 --profile minimal --component rust-src
export CARGO_TARGET_DIR="$HOME/wasixv2-orch/logs/b2-exec/target"
export WASM_BINDGEN=/path/to/wasm-bindgen
lib/wasixv2/host/tests/browser/build.sh
nice -n 19 taskset -c 4-7 node lib/wasixv2/host/tests/browser/run.mjs "$CARGO_TARGET_DIR/browser"
nice -n 19 taskset -c 4-7 node --test lib/wasixv2/host/tests/browser/protocol.test.mjs
```

`CHROME` overrides the runner's `/usr/bin/google-chrome`. The runner serves assets
on localhost with COOP/COEP, uses a temporary browser profile, and removes it on exit.
A missing browser or required capability fails the run. No fixture needs public
internet access. All generated wasm/assets stay in `CARGO_TARGET_DIR`; do not commit them.

The debug recipe rebuilds std with atomics, bulk memory and mutable globals, imports
shared runtime memory, and runs `wasm-bindgen --target web`. Its runtime maximum is
256 MiB, independently of guest memory limits. Bindgen adds a guard page to the
linker's 4 MiB initial memory; `inspect.mjs` reads the generated limits and emits
`limits.js` for the harness instead of guessing the minimum. Each additional worker
gets a bindgen-allocated 1 MiB shadow stack and separate runtime TLS.

### Rust TLS cleanup

The pinned Rust std's `sys/thread_local/mod.rs:guard::enable` explicitly leaves wasm
TLS destructors uncalled. Bindgen's `__wbindgen_thread_destroy` frees TLS storage
and the allocated stack, but does not run that destructor list. `prepare-std.py`
copies rust-src under the target directory and adds a wasm/atomics-only C export
calling the existing `destructors::run` and `rt::thread_cleanup`. Its local compiler
wrapper points Cargo build-std at that copy. Installed toolchains remain unchanged.

Normal retirement calls `browser_retire` (engine teardown and std cleanup), then
bindgen destruction, then closes the worker. The first instance uses the linker's
stack (`__stack_alloc == 0`), so it cannot use bindgen's unconditional stack free;
it retires last and its remaining TLS storage goes with the entire discarded heap.
Never reuse a stopped heap. The browser test observes heap-owning TLS destructors
on every worker before termination. Removing the std hook makes that test fail.

## Integration boundary

Serve `bootstrap.js`, `coordinator.js`, and `worker.js` as ES modules alongside the
embedding crate's wasm-bindgen output. On the page, asynchronously call:

```js
import { startCoordinator } from './bootstrap.js';
const coordinator = await startCoordinator({
    module,                     // compiled threaded runtime, not a guest module
    memory,                     // shared runtime memory with artifact-derived limits
    bindingsUrl,                // embedding crate's wasm-bindgen web module URL
    workerUrl,                  // optional override for worker.js
    configure: 'configure',     // optional synchronous Rust bootstrap export
    taskLimit: 64, serviceLimit: 4,
    prewarm: 2, servicePrewarm: 2,
}, onCoordinatorMessage);
```

Only the coordinator creates runtime workers. Its `ready` acknowledgement means
bootstrap/TLS and prewarmed resources are ready; no kernel work has run on the page.
Use `entry` messages to invoke a synchronous export on the root worker. An entry
must not return a Promise. SDK-facing process/package APIs remain SDK/B8 work.

On each runtime worker, `configure` should install `ObjectHooks` using the existing
engine `prepare_shared_object_message`, `receive_shared_object_message`, and shared
object collection functions. `prepare` captures an opaque envelope locally; the
receiver imports it before acknowledging readiness. `collect` runs after each task,
and `teardown` releases all remaining worker-local roots before std TLS cleanup.
The smoke uses a guest Memory envelope; host has no dependency on a wasm engine.

`BrowserExecutor::new(task_limit, service_limit)` runs only on a bootstrapped worker.
Use its task view for exec; expose `.services()` from the kernel's Host for timer/TTY
ThreadStarts. These views share admission state but reserve independent capacity.
A service admission failure is fatal and cannot enter the existing kernel fallback
onto `spawn_local`. B8 must wire these distinct Host executor views; B2 does not
change kernel/exec traits or edit their files.

For B10 synchronous imports, `browser::block_on(future)` polls on the calling worker
and parks on Pending. It supports nested drives and polls worker-local detached
futures without holding a registry lock or RefCell borrow across polls. Wakers set
shared readiness/sequence words and notify the owning worker; they never call a
foreign wasm-bindgen future queue. Local detached tasks are bounded at 1024;
`try_spawn_local` reports exhaustion, while the infallible trait method fails hard.
`Parker::{sequence,park,notify}` exposes the primitive separately. Snapshot the
sequence **before** polling/registering and use that snapshot to park after Pending.

Promises, fetch, module instantiation and JS timers must finish before dispatch or
run on independent I/O resources. A blocking service ThreadStart is not a JS event
loop: do not await WebHost's setTimeout future there. B4/B5/B6/B10 own those drivers.
Guest futexes remain kernel futexes; runtime parking words are never guest addresses.
Linux authorities for the ordering are `kernel/sched/wait.c:prepare_to_wait_event`,
`kernel/sched/core.c:try_to_wake_up`, and `kernel/kthread.c:kthread_create_on_node`.

## Admission and lifetime

A bounded reservation owns a boxed Send ThreadStart in the shared Rust heap. Before
acceptance, bootstrap failure, envelope rejection or timeout drops it exactly once.
The coordinator acknowledges readiness only after worker instantiation, TLS setup
and envelope import. The source then publishes acceptance and commits dispatch.
A never-reused ID can be claimed exactly once; delayed messages cannot claim a
replacement closure. No raw JS slab index crosses through shared Rust state.

All occupied task slots fail promptly at the configured limit; a child never queues
behind a parent it must unblock. Below that limit, the coordinator grows the pool
even while both the caller and its parent are blocked. Service capacity stays
independent. Atomics compare/exchange arbitrates readiness versus cancellation.

An accepted worker trap or unexpected death poisons the runtime and terminates all
its workers: shared locks/allocator state cannot safely be reused. The page receives
`fatal`, so it must fail all Machines associated with that runtime. For normal
shutdown, first stop Machines, wake/drain tasks and output, and drop their owners;
call `BrowserExecutor::close` to refuse new work. Send `shutdown` only when workers
are idle. `shutdown-busy` refuses premature teardown; `stopped` acknowledges that
all workers retired. Abrupt termination is infrastructure failure, not Linux exit.

## Coverage and follow-ups

The harness tests two workers sharing an Arc and guest SAB, separate runtime TLS,
foreign Wakers, parked-parent child admission, service progress with all task slots
occupied, idle reuse, admission errors, accepted traps, TLS destructors, normal
shutdown and page heartbeat. Native tests cover wake-before-wait, ownership,
capacity, nested local drives and bounded queues; Node tests cover coordinator races.

This is not a full guest/facade parity result. B3 supplies guest/foreign memory
operations; B4 supplies worker-safe timers; B8 wires Host views and engine hooks;
B10 connects synchronous guest imports. Actual kernel timer/TTY end-to-end runs,
guest signals/fork/I/O, additional browsers, and browser CI belong to those packages
and B11. No full wasixv2 suite is needed to run this host harness.
