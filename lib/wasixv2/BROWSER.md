# Browser embedding and SDK handoff

The browser target is `wasm32-unknown-unknown` with Wasmer's JS backend. Build
`wasixv2` with `--no-default-features --features browser,wasix1`. The SDK remains a
separate project: this repository supplies Rust embedding APIs and version-matched
worker assets, not the classic or current SDK's JavaScript public API.

Full SDK parity is **not signed off**. The checklist below distinguishes available
runtime mechanisms from missing SDK integration and unproved browser acceptance.
Evidence is scoped to the cited test; native model tests do not establish browser
transport behavior. The inventory covers classic SDK v0.6.0 (`94354975f119`) and
current SDK snapshot `c352b08566e1`, as audited in the browser plan.

## Try the source example

The existing [Rust embedding](api/examples/browser-facade.rs) assembles a Machine,
mounts, bounded streams and the browser Run handle. The companion
[page](api/examples/browser/index.html) and [runProgram](api/examples/browser/run.js)
show acquisition, invocation, output drain and worker shutdown from JavaScript.
It runs raw WASIX bytes; it is an example binding over the Rust facade.

Build and serve it using task-owned output directories:

```sh
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/absolute/task-owned-target
export WASM_BINDGEN=/path/to/wasm-bindgen-matching-Cargo.lock
bash lib/wasixv2/api/tests/browser/build.sh
npm ci --prefix tests/wasixv2/browser
node lib/wasixv2/api/examples/browser/serve.mjs \
  "$CARGO_TARGET_DIR/browser" /absolute/source-built-fixture-export
```

Open the printed `/example/index.html` URL. A module URL such as
`/fixtures/cases/2/main.wasm` must match the export manifest's asset paths
(the reviewed export places `base-exit-status/return` there). Other fixtures may
require their prescribed arguments, environment or mount files. `runProgram` accepts args, env pairs,
stdin text and cwd. Its result contains exitCode plus stdout/stderr byte arrays.
The example captures at most 16 MiB per output and uses a 30-second runtime
deadline after fetch/compile; acquisition cancellation and production
capture/streaming policy belong to the SDK.

The build recipe pins nightly with rust-src, rebuilds threaded std in an isolated
source tree, checks the exact wasm-bindgen version and inspects shared runtime
memory/TLS destruction. It changes no rustup default. Never distribute generated
Wasm from a different commit alongside these assets.

## Deployment and ownership contract

Serve a secure context with `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`; ensure CORS/CORP for other origins.
Allow the worker/module URLs in CSP. SharedArrayBuffer, Wasm shared memory and
worker `Atomics.wait` are mandatory. The required lane removes JSPI from both
page and workers; JSPI is optional and its optimisation lane is not signed off.
Modern Wasm exception handling is required by EH guests; fork requires a guest
built with asyncify independently of JSPI.

The page owns fetch, presentation and the asynchronous coordinator handle.
Only the coordinator creates workers. Task workers execute synchronous guest
calls and may block; fetch, WebSocket and Promise-producing providers must make
progress elsewhere. Kernel drivers have reserved workers and admission failure
is fatal. The pure JS memory-copy worker must never instantiate runtime Rust.
All runtime workers share one Rust heap; CLONE_VM shares guest memory, whereas
fork creates a copied guest memory. Neither Store nor Instance crosses workers.

Bundle the generated runtime.js/runtime_bg.wasm, bindgen snippets, limits.js,
bootstrap.js, coordinator.js, worker.js, timer-driver.js and memory directory
from one build. Include io-driver.js when using the classic WebSocket relay.
Call `configure_browser` on every runtime worker before object-envelope import.
[Assembly details](api/tests/browser/README.md) describe BrowserHost services,
network injection, package bytes and shared directory mounts.

`browser::spawn` publishes a task before returning `Run`. Each sandbox should
own its Machine: `Run::kill` shuts down that Machine's complete process tree.
`Run::wait` supports repeated/concurrent waits; dropping an unfinished Run
requests SIGKILL. Consume both outputs concurrently, wait for process exit and
remaining descendants, call `Session::finish`, and drain both outputs to EOF
before reporting completion. Await idle admissions and normal coordinator
shutdown before terminating workers. Infrastructure failure rejects every waiter
and discards the complete shared heap; it is never a successful process exit.
Linux authorities for the unchanged lifecycle are `kernel/fork.c:kernel_clone`,
`kernel/sched/core.c:wake_up_new_task` and `kernel/exit.c:do_exit,exit_mm`.

## Capability checklist

“Done” describes the stated Rust/runtime surface. “Missing” identifies scope in
the follow-up table. SDK wrapper compatibility remains missing for every row
until the separate SDK port's classic/current API tests pass.

| Classic/current capability | Facade/driver destination | Runtime status and evidence | Missing work |
| --- | --- | --- | --- |
| Raw module, WAT conversion | `load`, `Machine::run_module`, `browser::spawn`; SDK uses `wat` | Done raw: browser `base-exit-status` five variants, `base-args-env`; WAT used by `lifecycle_smoke` | SDK conversion/public run wrappers (S1) |
| Registry/local WEBC, named/default commands, command binary | `Package::from_bytes`, `entrypoint`, `commands`, `binary`, `prepare_command`, `install_packages`, `Machine::run_file` | Done bytes/commands: `package::tests::{byte_backed_packages_install_commands_without_native_sources,version_two_sources_migrate_from_bytes,initial_command_configuration_preserves_explicit_values,supplied_dependency_atoms_survive_the_package_sources}`; Missing browser WEBC acceptance | S1, S2, R1 |
| args/env/cwd, extra packages, sandbox install/shell | `ProcessConfig`, `set_working_dir`, `install_packages` | Done args/env: `base-args-env`; package precedence tested above; Missing integrated browser cwd/package/shell tests | S1, R1 |
| Multiple instances/shared runtime/cache/pool, close/disposal | `browser::builder`, `BrowserExecutor`, Machine, `Run` | Done isolated kernels/shared Directory and B13 same-runtime 500/resource bounds; Missing Chromium complete reclamation and SDK multi-client cache/pool disposal acceptance | S1, S2, R2 |
| stdin/stdout/stderr, wait and captured output | `Session`, `Streams`, `Run::wait` | Done bounded independent outputs/EOF/drain: `lifecycle_smoke.backpressure` (256 KiB through 64 KiB queues), adapter.test.mjs final-result ordering | SDK streams/capture limits/lines (S1) |
| Live spawn/capture/pipe/discard, input close, timeout/terminate/kill | `browser::spawn`, `Run::{wait,kill,signaller}`, stdio-web queues | Done kill/repeated concurrent wait/signal and B13 canceled wait/kill/deadline stress; Missing public terminate/deadline/pipe/capture API compatibility | S1, R2 |
| TTY, raw input/echo/editing/Ctrl-C/resize | Machine console, `Signaller::{console_resize,console_hangup}`, Session | Done busy browser Ctrl-C: `lifecycle_smoke.busyTty`; native `stdio-web/tests/tty.rs` covers canonical/raw, resize, foreground selection and hangup; Missing full browser TTY matrix | S1, R3 |
| Mutable shared Directory and mounts | `browser::directory`, WebFs, `populate_directory`, `Machine::mount` | Done shared bytes across Machines: lifecycle; `directory::tests::{readonly_directories_bytes_and_symlinks_keep_their_identity,escaping_paths_and_symlink_parents_fail_before_mutation}`; browser fs-rw-pread/readdir/rename/link-symlink/openat2-in-root | S1, R1 |
| Editing filesystem after sandbox creation | `Machine::vfs`, explicit `Ctx`, credentials/root and VFS operations | Done same provider identity: lifecycle; Missing browser editing API round-trip and credential-error acceptance | S1, R1 |
| Host mounts/storage provider | WebFs object-ID Provider protocol; memory/package/overlay drivers | Done memory provider: `lifecycle_smoke.sharedDirectory` and `directory::tests::readonly_directories_bytes_and_symlinks_keep_their_identity`; Missing JS-backed OPFS/File System Access integration; native host paths unavailable | S3 |
| pthreads/worker concurrency | `BrowserExecutor::spawn_thread(ThreadStart)`, engine object envelopes | Done reviewed `thr-atomic-wait-signal`, B13 lifetime/limit/fault lanes and admission/retirement models; Missing Chromium disposal/fault reclamation | R2 |
| sleep/poll/futex/atomic wait | BrowserHost off-worker timer, kernel futex/WaitQueue, runtime Parker | Done base-nanosleep/timerfd, thr-atomic-wait-signal and B13 cancellation/notify-before-wait/delayed-driver stress; Missing real background-tab scheduling acceptance | R2 |
| SAB/bootstrap/worker URL | `startCoordinator`, configure_browser, threaded std/TLS, C1 pure JS copy worker | Done artifact inspection + isolated page/worker capability probes; Missing distributable SDK worker URL/bundler/version contract tests | S1, R2 |
| Classic relay TCP/UDP/DNS | `browser::relay::connect`, supplied Net, vnet-web Classic Driver | Done actual browser TCP `relay_smoke`; native `vnet-web/tests/relay.rs:reference_relay_tcp_udp_dns_ipv4,reference_relay_tcp_udp_dns_ipv6` checks reference wire TCP/UDP/DNS; Missing browser UDP/DNS and stock-provider error/EOF semantics | R4, N1 |
| WISP egress/endpoint replacement | `browser::network`, `Transport`, `Capabilities::wisp` | Done provider capability contract: `vnet-web/tests/backend.rs:provider_capabilities_and_dns_answers_are_explicit_and_bounded`; Missing real WISP JS provider and browser reconnect/endpoint tests | S4, R4 |
| Loopback/peer TCP/browser HTTP ingress/ports | supplied Net, vnet-switch, `InboundSink`, `Publication` | Done native loopback: `api/tests/net.rs:loopback_tcp_carries_data_between_two_sockets`; publication model: `vnet-web/tests/backend.rs:publication_without_guest_listener_accepts_and_resets_instead_of_stalling`; Missing cross-Machine switch (vnet-switch is a placeholder), browser peer/HTTP ingress and SDK listener discovery | S5, R4 |
| v1 fetch HTTP import | wasix1 embedder provider (B9) | Missing on B12's B8 base; B9 owns implementation and acceptance independently | B9 |
| Registry URL/auth/cache/progress/cancellation | byte-backed Package/Loader boundary | Done accepts supplied bytes/dependencies; Missing SDK fetch/auth/CacheStorage/dedup/progress/AbortSignal | S2 |
| Feature probing/logging/bundling | capability probes, coordinator fatal/report messages | Done no-JSPI lane probes/artifact checks; Missing SDK logging/init APIs and bundled worker/CSP failure tests | S1, R2 |
| Fork/vfork/exec guest lifecycle | existing asyncify exec state machine and dedicated worker executor | Done reviewed proc-fork/proc-exec fixtures in B8; Original intermittent B8 bounds trap unreproduced after 105/105 Chromium mixed-fixture and 50/50 focused fork passes; no fix claimed | R2 |

## Follow-ups and limits

Every missing item above is assigned scope here. A native model or transport
smoke cannot close a browser acceptance item.

| ID | Owner/scope and completion criterion |
| --- | --- |
| S1 | Separate SDK: preserve classic `Runtime/Wasmer/Command/Directory/Instance` and current client/sandbox/package/process APIs, module/WAT helpers, stream/capture/line adapters, timeout/terminate/kill, terminal widgets, logging, worker URLs/bundlers/CSP and close/disposal. Run both snapshot API compatibility suites against this facade. |
| S2 | Separate SDK: registry resolution/dependency acquisition, auth/URLs, fetch+CORS, persistent/module caches, load deduplication, progress and AbortSignal. Supply retained byte-backed packages/loader cache to task workers without Promise dependencies on those workers. Test cancellation, auth/CORS failure and cache reuse. |
| S3 | Separate SDK/provider: OPFS/File System Access and host-storage bridge with explicit capabilities, owned object IDs and independent event-loop progress; test inode identity, open-unlink/rename, readonly/permissions and concurrent append. Optional persistence is not required for classic Directory parity. |
| S4 | Separate SDK/provider: WISP TCP/DNS callback transport on independent I/O worker, endpoint replacement/auth/DoH+CORS; test refusal, disconnect while blocked, reconnect without byte replay. Preserve explicit lack of WISP UDP/ingress. |
| S5 | Separate SDK plus narrow facade follow-up if required: service-worker HTTP framing/ingress, browser TCP bridge, peer sandboxes and read-only listener discovery. The vnet-switch crate currently contains only documentation; implement and test cross-Machine address routing before claiming peer networking. Test backlog saturation/publication lifetime and browser port APIs without claiming native host listeners. |
| B9 | Parallel wasix1 package: integrate B9 branch and run real v1 static/thread/spawn/fork/TTY/socket plus HTTP-provider/import-layout/CORS/failure acceptance. No v1 quirks enter kernel policy. |
| R1 | Runtime acceptance: source-built browser WEBC v2/v3/default/named/dependency/subdirectory command execution, cwd and extra packages, live VFS edits with credentials and provider failure cases. Existing native tests remain useful but insufficient. |
| R2 | B13 adds real limit/bootstrap/death, canceled wait/kill/deadline, notify-before-wait, timer cancellation/delayed-driver and repeated relay tests. Firefox lanes reclaim all workers. **Open:** Chromium normal shutdown and some timer-death cleanup retain workers; real background-tab scheduling remains unmeasured. See B13 evidence below; complete reclamation/signoff is withheld. |
| R3 | Runtime TTY acceptance: browser canonical/raw/echo/editing/EOF, resize/SIGWINCH, foreground job control and hangup under blocking/busy guests; reuse kernel behavior and native tty tests. |
| R4 | Runtime network acceptance: browser classic UDP+DNS/IPv6, TCP half-close/reset, queue bounds, disconnect/refusal/provider absence, ingress/peer networks and WISP adapter. Use deterministic local services. |
| N1 | Classic relay provider: emit distinguishable EOF/reset, preserve empty UDP and host-send errors, reverse/raw DNS provider, UDP publication, urgent data and port-zero publication where claimed. Current stock relay limits are detailed in vnet-web/README.md; unsupported capabilities must remain explicit. |
| R6 | B13 fixes copy/task-realm native-memory retention, Store host-import capture lifetime and the procfs mount ownership cycle. Chromium completes 500 same-runtime fork fixtures with bounded heap/RSS/reservations/actual worker targets and no forced GC; shutdown remains R2-open. Browser/Playwright owner: reduce the intermittent WebKit page/context crash (reproduced without the WASIX runtime); retain both failing default and explicit process-isolated evidence. Do not classify it as a fixed kernel fault. |
| CI1 | Integration CI: execute the cold source-toolchain producer with required private-source access, verify cache restore and all mandatory browser jobs. Local installed-toolchain runs do not prove cold provisioning. |
| P1 | Platform/approved-design follow-up: native coherent separately mapped shared pages, page protection/guard faults, native CoW and shared-page-backed AIO/io_uring rings are unavailable on JS. Preserve copied-mapping/retained-copy semantics; changing those requires a separately approved memory design. |

The JS backend cannot enforce guest `mprotect` or alias ranges across separately
forked linear memories. Same-SAB threads are coherent; copied mappings synchronize
at the existing defined points. Native shared-page facilities remain unavailable.
CPU-time accounting uses active wall time where native per-thread CPU clocks do
not exist. Background-tab scheduling can delay timers; it must never complete a
sleep early. These limits remain part of the embedding promise.

## Reproducing browser evidence

Use the source export manifest and one fresh artifact directory per invocation:

```sh
node tests/wasixv2/browser/run.mjs --browser firefox \
  --manifest /exports/manifest.json --runtime "$CARGO_TARGET_DIR/browser" \
  --artifacts /results/firefox
BROWSER=webkit node lib/wasixv2/api/tests/browser/lifecycle.mjs \
  "$CARGO_TARGET_DIR/browser"
CHROME=/usr/bin/google-chrome node tests/wasixv2/browser/run.mjs \
  --manifest /exports/manifest.json --runtime "$CARGO_TARGET_DIR/browser" \
  --filter proc-fork/ --repeat 100 --artifacts /results/fork-stress
```

The runner logs exact browser versions and page/worker capabilities, forces JSPI
absence in every served module and fails missing required capabilities. It never
imports native per-test expected-failure baselines. The original B8 intermittent fork bounds trap is recorded as **unreproduced**,
with no fix claimed, after 105/105 mixed fixtures and 50/50 focused fork runs.
It is not being investigated further in B12. Full signoff requires the missing
acceptance items above and both separate SDK API suites.

## Browser matrix and signoff (2026-10-30)

All fixture lanes use the same reviewed, source-built 21-case export and remove
JSPI in page and worker modules. These are the measured versions, not a promise
about every browser version or the SDK API. Required capabilities fail explicitly;
no fixture was skipped or blessed as an expected failure.

| Engine / lane | Fixture result | Lifecycle and deterministic relay TCP |
| --- | --- | --- |
| Chromium 151.0.7922.108, separate context per case | 105/105 mixed-fixture stress (five repetitions), plus 50/50 focused fork | Pass |
| Firefox 157.0, separate context per case | 21/21 | Pass |
| Playwright WebKit 27.2, separate context per case | **18/21**, page creation failed in three cases | Pass |
| Playwright WebKit 27.2, explicit `--fresh-browser` | 21/21 | Separate same-page lifecycle test passes |

WebKit context reuse is an **engine/Playwright host-environment failure**, not a
WASIX guest trap: `browserContext.newPage` failed before fixture instantiation
for fs-rw-pread, fs-readdir and proc-fork, and later cases still ran. The prior
session reproduced page/context crashes in `--smoke --repeat 5`, whose
`transport.js` only instantiates the tiny transport module and never loads the
WASIX runtime. A later five-case transport rerun passed, so the failure is
intermittent. Its exact browser/host cause is unconfirmed. Process isolation is
an explicit alternate diagnostic lane, not evidence that context reuse is fixed.
Keep the failing default lane visible; browser/harness ownership should reduce
it against the pinned Playwright/WebKit installation before removing this limit.

The original B8 fork **runtime bounds trap is unreproduced**, separately from
WebKit's pre-execution failures and R6's same-runtime allocation limit. Chromium
completed the 105-case stress and 50 focused fork runs; no correction or root
cause is claimed, and B12 does not investigate it further.

Evidence artifacts are retained under
`~/wasixv2-orch/logs/b12-signoff`: `subset-stress`, `fork-baseline`,
`final-firefox`, `final-webkit-default`, `final-webkit-process`, and
`final-webkit-context`, each with results/capabilities and per-case logs.
`smoke-webkit-repeat` preserves the earlier transport-only failure.
`final-{chromium,firefox,webkit}-lifecycle.log`, `final-example.log` and
`baseline.json` preserve the other acceptance results. The clean runtime was
rebuilt from the B8 facade base d997e5bb27e plus B12 source; no diagnostic root
counters, forced GC, stack canaries or temporary exports are shipped.

Scoped validation passed: 67 native facade unit/integration tests plus one
doctest, 26 facade/protocol JS tests, 11 runner tests, strict Makefile-flags
native all-target and wasm library/example clippy, workspace formatting and
the browser normal-dependency audit. No full wasixv2 suite was run.

**Signoff withheld:** the default WebKit lane is not green, B9 v1 acceptance is
separate, and the SDK wrappers/integrations and scoped runtime acceptance gaps
above remain. Passing Rust tests or the process-isolated lane cannot establish
classic/current SDK API compatibility.

## B13 lifetime and robustness (2026-10-30)

The lifetime quota is addressed in the measured 500-run lane. Complete R2 signoff
is **withheld**: Chromium still retains its coordinator after normal shutdown,
and the final timer-death lane also retained its failed timer worker.

C1 unregister removes a JS root but does not force an isolate to collect dead
Memory wrappers. The copy service now hands live state and endpoints directly to
a replacement pure-JS realm after eight unregisters. Firefox requires readiness
on that handoff port and sender survival through restored-state acknowledgement;
normal retirement closes the service's own ports on its own event loop. Fatal
uncertainty still poisons the heap and forcefully terminates workers.

Task-only GC diagnostics exposed a second isolate-lifetime dependency after the
copy fix. A task realm is now refreshed on its next admission after 32 starts,
only when `browser_can_retire` reports no live shared engine objects. Existing
Rust/TLS retirement precedes replacement bootstrap; local futures already drain
before idle. Missing readiness disables refresh, and root/service workers are
retained. Cancellation, capacity and retirement-failure models cover this path.
Consumed dispatch envelopes are released immediately after cloning. Store-owned
host-import callbacks release Rust captures without JS finalization, and procfs
no longer retains the unused initial mount namespace through its fallback view.
These changes preserve C1/C2, guest ABI and Linux mm/task ownership.

All rows use the source-built debug artifact, B8 coordinator and B11 isolated
server/expectations, with JSPI absent. No acceptance lane invokes GC.

| Lane | Measured result |
| --- | --- |
| Chromium 156.0.8078.4, one runtime, 500 proc-fork fixtures | 500/500 fixture assertions; 5,000 child address spaces; resource bounds pass; **shutdown fails** with one coordinator target |
| Chromium continuing-parent fixture | 500 isolated forks/reaps complete; **shutdown fails** |
| Firefox 157.0, one runtime, 60 proc-fork fixtures | Pass, including zero workers; 74 copy and 17 task retirements |
| Firefox continuing-parent fixture | 500 isolated forks/reaps and final ECHILD pass, including zero workers |
| Limit/rejected-start ownership | Chromium five repetitions complete assertions but fail shutdown; Firefox five repetitions pass |
| Cancellation/deadline/timer/notify-before-wait | Chromium three repetitions complete assertions but fail shutdown; Firefox two repetitions pass |
| Bootstrap/accepted-task/timer death | Chromium bootstrap/task pass; timer cleanup fails in the final run; Firefox all three pass |
| Existing relay, 50 connections/sends in one runtime | Chromium completes relay/I/O assertions but fails shutdown; Firefox passes with zero workers |
| Timer callbacks delayed 50 ms | Chromium three repetitions satisfy deadline/no-early-completion assertions but fail shutdown |
| Existing lifecycle/shared-directory/signal/TTY and relay smoke | Chromium and Firefox assertions pass; this does not replace the disposal gates above |

The final Chromium 500-run samples show a 71.625 MiB shared-heap maximum and
10.0625 MiB range after iteration 25, 15 Rust workers, at most 18 actual worker
targets (bound 26), 1.27 GiB renderer RSS maximum, and 8.07 MiB virtual-reservation
range. It performs 625 copy and 164 guarded task retirements. The fixed gates are
16 MiB heap range, a fixed Rust pool, bounded actual targets, 2 GiB renderer RSS
and 16 GiB reservation range. RSS includes browser/JIT costs. Later peak growth
is diagnostic; earlier 8 MiB heap and warm-relative RSS budget failures remain
preserved. Firefox's RSS/reservations were not measured. Delayed timer injection
does not establish real background-tab scheduling behavior.

`tests/wasixv2/browser/worker-close.mjs` preserves a separate failing reduction:
the real coordinator with fake JS boot/retire workers closes for an empty Module
but retains its Chromium target for the large compiled runtime Module, which is
**never instantiated**. Firefox closes both. Raw-CDP control without attached
worker debuggers also retained the coordinator. The coordinator/browser ownership
cause remains unconfirmed; no debugger-only classification, forced GC, timeout
waiver, process-isolation fallback or creator-topology change is shipped.

Reproduce against a reviewed source export containing `proc-fork-lifetime`:

```sh
node tests/wasixv2/browser/lifetime.mjs /runtime /export/manifest.json /results/lifetime 500 chromium
node tests/wasixv2/browser/lifetime.mjs /runtime /export/manifest.json /results/firefox 60 firefox
node lib/wasixv2/api/tests/browser/relay-lifetime.mjs /runtime /export/manifest.json /results/relay 50
node tests/wasixv2/browser/worker-close.mjs /runtime /results/close-control chromium
```

Evidence is under `~/wasixv2-orch/logs/b13-lifetime/acceptance-*`,
`reduction-{chromium,firefox}`, and `STATE.md`. Earlier failed 500-run reports,
237/238 child-output failures before guarded reuse, rejected prototypes and GC
diagnostics remain separate. Failed result JSON includes the actual process
output; a completed fixture count never overrides resource/disposal failure.
Scoped native tests, strict native/wasm clippy, real-threaded-std owner-readiness
unit, 65 runtime JS models, 26 runner tests, formatting and feature audit pass.
No full wasixv2 suite was run; SDK API/worker packaging and the other follow-ups
above remain separate acceptance work.

## Chromium debug baseline (2026-10-30)

Measured locally on Chromium 151.0.7922.108, with JSPI removed, CPUs 4–7 and
nice 19. This is the required unoptimised threaded debug artifact, not a release
size or performance promise. The shared runtime heap maximum is 256 MiB;
allocated task-worker shadow stacks are 2 MiB. Browser/network startup is local
loopback with Cache-Control: no-store; worker tests reuse the compiled Module.

| Measurement | Observed |
| --- | --- |
| Runtime module fetch plus bootstrap module imports | 188.7 ms |
| WebAssembly.compile | 80.6 ms |
| Coordinator readiness, 0 / 2 / 8 prewarmed task workers | 130.1 / 147.9 / 241.2 ms |
| First guest invocation after readiness (2 prewarmed tasks), including Machine boot and output drain | 227.0 ms |
| getpid import → kernel → guest, five samples of 20,000 calls | median 8.75 µs/call; min 7.98, max 18.51 |
| Shared runtime heap at readiness, 6 / 8 / 14 runtime workers | 14.375 / 18.5 / 30.875 MiB |
| Incremental shared runtime heap per added worker | 2.0625 MiB |
| Debug runtime Wasm file | 164,833,832 bytes |

Readiness includes the anchor, off-worker services and pure-JS memory-copy
worker. The runtime-worker counts exclude that copy worker. Heap growth is
counted once for the shared heap; the per-worker estimate includes stack/TLS
and allocator/page rounding. It excludes guest memory, JS objects, compiled
code, engine reservations and process RSS, so it is **not total worker memory**.
A browser worker shares an OS process with other workers; per-worker RSS was not
measured. SDK integration should add long-lived pool and total-memory baselines
under its own workload (R2/R6).

The syscall interval uses guest CLOCK_MONOTONIC bookends and includes the loop,
loader preemption and kernel return hook; it sends no JS message per syscall.
The spread includes engine warmup. Fetch, compile, ready and first-run numbers
are separate measurements, not percentiles or a single contiguous cold start.

Reproduce with the same source build, then retain the emitted JSON:

```sh
CHROME=/usr/bin/google-chrome node lib/wasixv2/api/tests/browser/baseline.mjs \
  "$CARGO_TARGET_DIR/browser" /results/baseline.json
CHROME=/usr/bin/google-chrome node lib/wasixv2/api/tests/browser/example.mjs \
  "$CARGO_TARGET_DIR/browser" /exports/manifest.json
```

`baseline-data.test.mjs` checks normalisation and shared-heap accounting; the
browser run requires successful guest exit and exactly 32 timing bytes for the
warmup and all five samples. The example regression checks exit/atexit output,
invalid runtime URL and missing-module failure against the real source fixture.
