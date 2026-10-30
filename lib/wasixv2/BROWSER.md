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
| Registry/local WEBC, named/default commands, command binary | `Package::from_bytes`, `entrypoint`, `commands`, `binary`, `prepare_command`, `install_packages`, `Machine::run_file` | Done bytes/commands: `package::tests::{byte_backed_packages_install_commands_without_native_sources,version_two_sources_migrate_from_bytes,initial_command_configuration_preserves_explicit_values,supplied_dependency_atoms_survive_the_package_sources}`; browser R1 WEBC v2/v3 default/named/dependency/extra commands | S1, S2 |
| args/env/cwd, extra packages, sandbox install/shell | `ProcessConfig`, `set_working_dir`, `install_packages` | Done args/env: `base-args-env`; package precedence tested above; browser R1 cwd/package and R3 mini-shell acceptance | S1 |
| Multiple instances/shared runtime/cache/pool, close/disposal | `browser::builder`, `BrowserExecutor`, Machine, `Run` | Done isolated kernels/shared Directory: `lifecycle_smoke.sharedDirectory,machineIsolation`; Missing multi-client cache/pool disposal acceptance | S1, S2, R2 |
| stdin/stdout/stderr, wait and captured output | `Session`, `Streams`, `Run::wait` | Done bounded independent outputs/EOF/drain: `lifecycle_smoke.backpressure` (256 KiB through 64 KiB queues), adapter.test.mjs final-result ordering | SDK streams/capture limits/lines (S1) |
| Live spawn/capture/pipe/discard, input close, timeout/terminate/kill | `browser::spawn`, `Run::{wait,kill,signaller}`, stdio-web queues | Done kill/repeated concurrent wait/signal: `lifecycle_smoke.kill,wait,signal`; Missing public terminate/deadline/pipe/capture API compatibility | S1, R2 |
| TTY, raw input/echo/editing/Ctrl-C/resize | Machine console, `Signaller::{console_resize,console_hangup}`, Session | Done busy browser Ctrl-C: `lifecycle_smoke.busyTty`; native `stdio-web/tests/tty.rs` covers canonical/raw, resize, foreground selection and hangup; browser R3 canonical/raw/echo/EOF, resize, job control and hangup acceptance below | S1 |
| Mutable shared Directory and mounts | `browser::directory`, WebFs, `populate_directory`, `Machine::mount` | Done shared bytes across Machines: lifecycle; `directory::tests::{readonly_directories_bytes_and_symlinks_keep_their_identity,escaping_paths_and_symlink_parents_fail_before_mutation}`; browser fs-rw-pread/readdir/rename/link-symlink/openat2-in-root and R1 live VFS acceptance below | S1 |
| Editing filesystem after sandbox creation | `Machine::vfs`, explicit `Ctx`, credentials/root and VFS operations | Done same provider identity: lifecycle; browser R1 held-inode round-trip, EACCES/EROFS and provider EIO acceptance below | S1 |
| Host mounts/storage provider | WebFs object-ID Provider protocol; memory/package/overlay drivers | Done memory provider: `lifecycle_smoke.sharedDirectory` and `directory::tests::readonly_directories_bytes_and_symlinks_keep_their_identity`; Missing JS-backed OPFS/File System Access integration; native host paths unavailable | S3 |
| pthreads/worker concurrency | `BrowserExecutor::spawn_thread(ThreadStart)`, engine object envelopes | Done reviewed `thr-atomic-wait-signal`; protocol.test.mjs admission/failure models; Missing extended lifetime/limit browser stress | R2 |
| sleep/poll/futex/atomic wait | BrowserHost off-worker timer, kernel futex/WaitQueue, runtime Parker | Done base-nanosleep/timerfd and thr-atomic-wait-signal; `host/src/timer.rs:expiry_coalesces_in_hard_expiry_order_without_firing_early,replacing_and_cancelling_a_timer_drop_wakers_outside_the_queue_lock`; Missing combined cancellation/background-tab browser acceptance | R2 |
| SAB/bootstrap/worker URL | `startCoordinator`, configure_browser, threaded std/TLS, C1 pure JS copy worker | Done artifact inspection + isolated page/worker capability probes; Missing distributable SDK worker URL/bundler/version contract tests | S1, R2 |
| Classic relay TCP/UDP/DNS | `browser::relay::connect`, supplied Net, vnet-web Classic Driver | Done Chromium/Firefox source network fixtures: classic TCP/UDP IPv4+IPv6, empty UDP/peek/trunc, A/AAAA and opt-in PTR/raw DNS; FIN/reset, host send errors and native urgent round-trip. Stock deployment fidelity remains opt-in | R4, N1 |
| WISP egress/endpoint replacement | `browser::network`, `Transport`, `Capabilities::wisp` | Done provider capability contract: `vnet-web/tests/backend.rs:provider_capabilities_and_dns_answers_are_explicit_and_bounded`; Missing real WISP JS provider and browser reconnect/endpoint tests | S4, R4 |
| Loopback/peer TCP/browser HTTP ingress/ports | supplied Net, vnet-switch, `InboundSink`, `Publication` | Done native loopback: `api/tests/net.rs:loopback_tcp_carries_data_between_two_sockets`; publication model: `vnet-web/tests/backend.rs:publication_without_guest_listener_accepts_and_resets_instead_of_stalling`; Done browser TCP+UDP ingress and two-Machine relay-mediated peer acceptance. Missing cross-Machine switch address routing (vnet-switch is a placeholder), SDK HTTP/service-worker framing and listener discovery | S5, R4 |
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
| R1 | Complete for the Rust/runtime facade: browser source-built WEBC v2/v3/default/named/dependency/subdirectory/cwd/extra-command and live VFS credential/provider-error acceptance, Chromium and Firefox. Evidence and reproduction below. SDK acquisition/cache/wrappers remain S1/S2/S3. |
| R2 | Runtime robustness: worker limit/bootstrap/death, same-runtime reuse/TLS/object collection, canceled wait/kill/deadline, notify-before-wait, timer cancel/background throttling, UI/I/O heartbeat, complete heap/driver/relay reclamation. Add stress and failure injection to actual supported browser lanes. |
| R3 | Complete for the runtime console: browser canonical/raw/echo/editing/EOF, resize/SIGWINCH, foreground job control and hangup under blocking/busy guests, reusing kernel behavior and existing PTY/shell fixtures. Chromium and Firefox evidence below; terminal widgets/wrappers remain S1. |
| R4 | B15: deterministic source fixtures cover classic UDP+DNS/IPv6, FIN/reset, bounded queues/overflow, disconnect/refusal/provider absence, fixed TCP+UDP ingress and two-Machine relay-mediated peers. Chromium/Firefox evidence below. Real WISP adapter and virtual-switch address routing remain S4/S5. |
| N1 | B15: explicit classic_fidelity profile carries distinguishable EOF/reset, empty UDP, host-send errors, reverse/raw DNS, fixed UDP publication and urgent marks. Native MSG_OOB test service proves both directions. Stock v1 server remains unchanged; exact additive records/server changes and deployment limits are in vnet-web/RELAY-FIDELITY.md. Port-zero and connected-host UDP ICMP/errqueue remain unsupported. |
| R6 | Runtime/engine lifetime: same-runtime proc-fork currently exhausts browser guest-memory allocation after roughly 22 runs despite C1 unregister and zero local root counters. Reduce live-root versus engine-GC/address-reservation behavior and prove long-running pool reuse without forced GC. Browser/Playwright owner: reduce the intermittent WebKit page/context crash (reproduced without the WASIX runtime); retain both failing default and explicit process-isolated evidence. Do not classify it as a fixed kernel fault. |
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

## R1/R3 browser acceptance (2026-10-30)

The tests repo's `b14-accept` branch (`fae0ded`) adds an exact 15-configuration selection in
`browser/accept-allowlist.txt`. Chromium 151.0.7922.108 and Firefox 157.0 each
passed **15/15**, with shared memory, cross-origin isolation and JSPI absent in
page and workers. Every guest and WEBC atom is built from source; the browser
constructs and parses actual WEBC v3/v2 bytes before VFS execution. No prebuilt
Wasm or expected-failure changes are committed.

R1 covers default/named launch, supplied dependency atoms and commands, extra
packages and command precedence, writable package/subdirectory mounts with
containment, default/explicit cwd and argument/environment precedence. A live
provider-backed directory checks guest-to-embedder writes and embedder writes,
rename/unlink/replacement while the guest holds an inode; explicit foreign
credentials give EACCES, readonly mounts EROFS and a test provider's lookup/read
failures EIO. Invalid package bytes and missing command/dependency fail explicitly.
The v2 producer lays its selected tree under the legacy atom-volume directory;
v3 uses an independent volume plus `host_path` selection.

R3 checks exact canonical echo/erase and Ctrl-D followed by further input, raw
control bytes without echo, caught SIGWINCH/all four winsize fields and hangup
under blocked/busy guests, foreground Ctrl-C after background SIGTTIN, and
default SIGHUP via input EOF or explicit hangup. Existing `tty-pty`,
`tty-pty-jobctl` and `tty-mini-shell` extend coverage to TOSTOP/SIGTTOU,
stop/continue and foreground swaps. Test actions wait for complete guest output
markers; all line discipline/job-control policy stays in the kernel.

Linux `drivers/tty/tty_io.c:__tty_hangup` installs hung-up file operations before
`tty_signal_session_leader`, so EOF can race ahead of SIGHUP. The source test
waits for eventual SIGHUP after EOF instead of assuming atomic publication.
No runtime defect was established and no kernel/exec contract changed.

Reproduce with the B8/B12 build recipe above, then in `tests/wasixv2`:

```sh
cargo run -p wasixv2-tests --locked -- --browser-accept-export /new/export
CHROME=/usr/bin/google-chrome node browser/run.mjs --manifest /new/export/manifest.json \
  --runtime /browser-runtime --artifacts /new/chromium-results
node browser/run.mjs --browser firefox --manifest /new/export/manifest.json \
  --runtime /browser-runtime --artifacts /new/firefox-results
```

Shared-machine builds use nice 19, CPUs 4–7, `-j3`, `CARGO_INCREMENTAL=0` and
owned debug targets. Evidence is retained under `~/wasixv2-orch/logs/b14-accept`;
`STATE.md` records exact source revisions and final artifact/result paths. This
closes the scoped R1/R3 runtime acceptance items; SDK API parity and the other
follow-ups above remain unsigned.

## B15 network acceptance

The source-built `browser/net-allowlist.txt` in tests branch `b15-net` adds 17
network cases to the B8/B11 runner. Each case creates fresh deterministic local
TCP/UDP/relay services; required IPv6 and Linux urgent service failures fail the
case. Chromium and Firefox use the same SAB-isolated no-JSPI lane and exact
expectations. No native expected-failure baseline or Wasm binary is added.

The tests found and fixed a relay publication race: accept credit issued while
no guest listener existed retained that decision and reset a later connection
after the guest had started listening. The driver now checks current sink
readiness when the accepted child arrives, outside its mutex. The new
`publication_rechecks_a_listener_created_after_accept_credit` regression fails
before the fix. Linux authorities are
`net/ipv4/tcp_ipv4.c:tcp_v4_syn_recv_sock` and
`net/ipv4/inet_connection_sock.c:inet_csk_accept`.

The additional provider unit tests preserve urgent ordering and receive marks,
publication attachment/lifetime, empty datagrams, asynchronous UDP errors and
accepted UDP sends across last close. A one-byte receive-buffer regression also
proves that skipping an urgent byte wakes the driver even when the read returns
EAGAIN, allowing its held frame to advance. The codec tests retain byte compatibility
with the stock wire and reject malformed versioned extension records.
[Provider contract and stock-server follow-up](vnet-web/RELAY-FIDELITY.md) define
what deployments must implement before selecting classic_fidelity. Port-zero
publication remains unsupported; no unresolved address is reported as bound.

Peer evidence is two separate Machines/Nets communicating over a relay
publication. It does not establish virtual-switch address routing, native host
socket options beyond the implemented records, connected-UDP ICMP/error-queue
support, or SDK HTTP/WISP wrappers. Those remain explicit S4/S5/provider work.

Measured network lanes (2026-10-30):

| Engine | Required no-JSPI cases | Maximum local relay host write queue |
| --- | --- | --- |
| Chromium 151.0.7922.108 | 17/17 pass | 16,384 bytes |
| Firefox 157.0 | 17/17 pass | 16,384 bytes |

The test provider enforces a 256 KiB host write-queue bound. Runtime socket queues
use the existing configured limits; the overflow case deliberately saturates the
worker receive bridge and must fail the socket with ECONNABORTED, never silently
lose transport state or report successful EOF. Evidence is retained under
`~/wasixv2-orch/logs/b15-net/{final-chromium,final-firefox}`; both contain
results, capabilities, per-case logs and service counts/maxima. Rebuild the debug
runtime and use `--browser-net-export` before rerunning. Full SDK/browser parity
remains withheld for the independent follow-ups above.

Scoped B15 checks pass: 35 provider tests (including the actual classic reference
relay on IPv4/IPv6), 68 native facade tests plus one doctest, 32 runner Rust unit
tests (two existing ignored tests), 18 runner JavaScript tests, facade adapter/I/O
unit checks, strict Makefile-flags native and wasm clippy, formatting and the
browser dependency audit. The full WASIX suite was not run.
