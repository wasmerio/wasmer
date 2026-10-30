# Browser embedding

See [browser support and SDK handoff](../../../BROWSER.md) for the runnable
JavaScript example, classic/current capability checklist, engine matrix and
Chromium debug baseline.

Build the facade with defaults disabled:

```sh
cargo check -p wasixv2 --target wasm32-unknown-unknown \
  --no-default-features --features browser,wasix1
python3 lib/wasixv2/api/tests/browser/check-features.py
```

`browser` selects the JS engine, B2 worker executor, B4 off-worker timers and
B5/B6/B7 network, filesystem and stdio drivers. `browser-drivers` exposes the
engine-independent drivers to native tests without enabling the JS engine.
There is no Tokio, mio, native VM or compiler in the browser runtime graph.
The normal native facade defaults remain unchanged; no CLI wasm port is needed.

The full lane requires a secure cross-origin-isolated page and SharedArrayBuffer.
The primary lane deliberately removes JSPI APIs. Runtime workers share one Rust
heap, with separate guest memories; only the coordinator creates workers.

## Host and Machine assembly

Use B2's asynchronous `startCoordinator` before entering Rust. Its `configure`
option must name `configure_browser` on every worker, so Wasmer's module/memory
envelopes are imported before a ThreadStart is acknowledged. Serve all generated
bindgen snippets and these version-matched assets from one runtime bundle:
`bootstrap.js`, `coordinator.js`, `worker.js`, `timer-driver.js`, and
`memory/{client,operations,copy-worker}.js`. Set `memoryCopyUrl` to that copy worker;
it is pure JS and never enters runtime Rust. B4's timer worker always starts
before runtime readiness and uses reserved capacity independent of guests.

On the owning Rust worker:

```rust,ignore
let executor = BrowserExecutor::new(64, 8)?;
let host = Arc::new(BrowserHost::new(executor.clone())?);
let machine = wasixv2::browser::builder(
    Arc::new(host.services()), executor.clone(), network,
).user(credentials).build();
```

The kernel Host uses reserved service workers; exec uses the task executor.
Both read the same Machine Timekeeper, including after root-layer assembly.
`MachineBuilder::network` also accepts a supplied `Arc<Net>` on native platforms.
Each Machine has its own kernel, task tree, private root and network namespace;
share directories explicitly by mounting the same `Arc<WebFs>` where desired.
A provider's own clock remains the clock chosen by its owner.

`browser::spawn` publishes a kernel task before returning a Run, then starts the
guest on a dedicated worker. `Run::wait` supports concurrent/repeated calls and
canceled waits unregister. `Run::signaller` routes signals, resize and hangup
through the existing kernel. `Run::kill` ends and waits for the Machine's entire
process tree; use a private Machine for one sandbox run. Dropping an unfinished
Run requests SIGKILL. Infrastructure worker failure revokes the whole heap and
the JS coordinator rejects waiters; it must never reuse a poisoned heap.

## Bytes, packages, directories and streams

Raw bytes use `load` and `browser::spawn`; an existing `Module` can be supplied
with its Engine. Compilation/instantiation are synchronous on the task lane.
Prefetch all Promise-based sources on the independent event loop first; an
embedder Loader must obey that rule for subsequent exec images too.

`Package::from_bytes(OwnedBuffer)` accepts WEBC v3 and migrates v2 in memory.
Supply resolved dependency aliases with `with_dependency`; registry auth,
fetch/cache/progress and persistent caching stay in the SDK. `install_packages`
installs the root plus additional packages through existing PackageFs/overlay
layers. The first command name wins; dependency volumes are underneath root
volumes. Subdirectory mappings resolve through VFS and use a directory as the
layer root. `binary` retains the original atom buffer. After installation,
`prepare_command` selects the entrypoint or a named command, prepares args/env/cwd
with explicit caller values winning, and returns the `/bin` path for `run_file`.
Nested v1 command defaults remain registered in wasix1, outside Linux policy.

`browser::directory` creates B6's shared memory provider. Use `populate_directory`
for a fresh mount's byte snapshot, or edit through VFS with explicit credentials
and root. Snapshots validate names and directory ancestry before mutation, retain
symlinks without following them, and apply readonly directory modes after data.
A provider mount can outlive any process. SDK providers can implement B6's owned
object-ID protocol; optional OPFS/File System Access policies remain external.

`browser::Session` yields Stdio and bounded Streams. Consume both outputs while
the guest runs; closing stdin is stream EOF, while a terminal receives raw Ctrl-C
and Ctrl-D through the kernel line discipline. After final exit, shut down any
remaining descendants, then call Session::finish while continuing to consume
outputs to EOF. Await downstream Web Stream writes before publishing JS success.
B7's `stdio-web/web-streams.mjs` adapts these endpoints to an SDK coordinator bridge.
The fixture adapter exercises this ordering with an independent service collector.

## Classic relay

Set bootstrap's `ioModuleUrl` to the bundled `io-driver.js`. The coordinator
creates that independent event-loop runtime worker before readiness. It owns
WebSockets and invokes short queue-completion exports, without ever parking or
running guest code. `browser::relay::connect` supplies Net plus a Relay owner;
B5's Classic codec/Driver runs on a reserved Rust service and exchanges bounded
whole frames with that I/O worker. No JS Promise needs a blocked task's event loop.

Only one outgoing frame is outstanding; WebSocket buffered bytes and received
frame bytes/records are bounded. A malformed/overflow/disconnected transport
fails its existing session without replay. Stop the Machine before awaiting
Relay::shutdown; this joins the driver and releases WebSocket callbacks. B5's
capabilities describe the actual relay (including the stock server's EOF/DNS
limitations); WISP/providers can instead use `browser::network` with their own
independent event-loop transport and explicit capabilities.

## Source-built browser gates

Use Node 22+ and B11's pinned Playwright installation. Never commit the output:

```sh
export CARGO_TARGET_DIR=/absolute/task-owned-target
export WASM_BINDGEN=/path/to/wasm-bindgen-matching-Cargo.lock
bash lib/wasixv2/api/tests/browser/build.sh
npm ci --prefix tests/wasixv2/browser
node --test lib/wasixv2/api/tests/browser/*.test.mjs \
  lib/wasixv2/host/tests/browser/protocol.test.mjs
CHROME=/usr/bin/google-chrome node lib/wasixv2/api/tests/browser/lifecycle.mjs \
  "$CARGO_TARGET_DIR/browser"
node tests/wasixv2/browser/run.mjs --manifest /source-built/export/manifest.json \
  --runtime "$CARGO_TARGET_DIR/browser" --artifacts /new/results
```

The wrapper reuses B2's pinned nightly/rebuilt std, shared-memory limits, TLS
cleanup, debug link and artifact inspection. Its inert index does not run B2's
separate host smoke. Lifecycle tests cover real busy-guest Ctrl-C, isolated
Machines/shared provider bytes, signal/kill/wait, output backpressure and classic
relay guest TCP against a deterministic local WebSocket fixture. That fixture
uses B11's lockfile-pinned Playwright WebSocket server utility; it owns no guest
network policy.

The reviewed B11 guest subset is the acceptance lane; missing capabilities and
any fixture failure fail CI. Native shared-page/MMU limits remain the existing
platform limits. Browser v1 HTTP compatibility, broader foreign-memory/TTY/network
fixture coverage, Firefox/WebKit and the separate SDK port/signoff remain their
assigned packages; these tests do not declare universal browser parity.

CI's separate producer uses `toolchain.env` source pins and
`prepare-toolchain.sh` to build debug tools and EH/asyncify C sysroots, then
exports only the reviewed subset. A cold source toolchain build is expensive;
its exact-pinned cache is independent of runtime artifacts and is saved before
fixture export, so an exporter failure does not discard the built tools. Only
executables, Clang resource headers and the two sysroots are cached; private
source checkouts and compiler object files are excluded. Host tools use debug
builds with assertions, without bulky debug symbols, and serialized LLVM links.
The cold runner installs native build prerequisites including lld and rsync
(Linux `Makefile:headers_install` uses rsync for the pinned UAPI headers).

`wasix-org/libc2` is private. Set the Actions secret `WASIXV2_TOOLCHAIN_TOKEN`
to a credential with **Contents: read** access to that repository (and any other
private pinned source/submodule). Until a dedicated credential is provisioned,
the workflow uses `COMPAT_TESTS_GH_TOKEN`, the existing private cross-repository
CI credential from `.github/workflows/patchsmith-test.yml`. A workflow's own
`github.token` is scoped to Wasmer and cannot fetch libc2. Fork pull requests do
not receive these secrets; their cold producer therefore fails explicitly and
requires a trusted same-repository run. Do not expose credentials to fork code
using `pull_request_target`.

The producer overrides inherited Git credential helpers for source fetches,
uses `GH_TOKEN` only from the environment, and never embeds tokens in URLs,
source configuration, generated env.sh or caches. It checks private access
before public downloads and reports the missing secret in the step log. A
restored completion marker is accepted only with all required tools/sysroot
files present. Runtime/producer/fixture jobs remain required, with per-test
logs retained on failure; no native expectation changes or prebuilt Wasm.

## WASIX v1 guest HTTP

With `wasix1`, `MachineBuilder::wasix1_http_provider` installs an explicit guest
capability retained across root assembly and exec. The historical canonical WAI
imports, handles and codec live in `wasix1`; native and browser facade entry points
validate legacy imports before task admission. This does not fetch packages.

For browser Fetch, install `wasixv2::wasix1_http::browser::FetchProvider` with bounded
request/response/in-flight limits and set bootstrap `ioModuleUrl` to `io-driver.js`
next to `http-fetch-driver.js`. The coordinator starts B8's independent I/O worker
before guests: owned commands start Fetch there and short completion exports wake
the existing v2 control path. Dropping the pending future aborts its request. No
Fetch Promise runs on the blocking guest worker. Origin, CORS, forbidden headers
and opaque-response restrictions remain enforced by the browser. Explicit bounded
redirect counts cannot be implemented by browser Fetch and return a capability
error; no-follow rejects unreadable opaque redirects. The default browser redirect
policy is Fetch's ordinary follow behavior. Installing this capability is separate
from granting the socket relay.
