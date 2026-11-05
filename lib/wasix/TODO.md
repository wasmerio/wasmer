# WASIX compatibility TODO

Stopped-work checkpoint: 2026-11-05. Integration branch: `feat/wasixv2`.
Subagents were stopped at the user's request. **Full supported v1 parity is not complete.**
This tree was renamed from `lib/wasixv2` to `lib/wasix`; crate names remain unchanged.
The old runtime remains in `lib/wasix-legacy` until its consumers have working replacements.

## Scope and snapshot

The delivery goal is everything that worked in supported v1 profiles working well
through v2 and its compatibility adapter, including custom bindings/NAPI. Completing
the entire Linux syscall surface is not required. Preserve original consumer tests;
compilation, model tests and feature flags alone do not establish compatibility.

The integration code snapshot is `2854c34dc590c5e832ba727036317af9e42d659f` plus
this documentation. It combines the delivered parity work, native NAPI frame/GC
repair and directory migration. The unfinished branches below are **not merged**
into that snapshot. Existing focused component evidence and offline locked Cargo
metadata passed; no fresh whole-snapshot release/browser/full-suite validation was
performed for this stop checkpoint. Source revisions are in [SNAPSHOT.json](SNAPSHOT.json).

| Repository | Source snapshot | Role |
| --- | --- | --- |
| [Wasmer](https://github.com/wasmerio/wasmer/tree/feat/wasixv2) | `feat/wasixv2`, code `2854c34dc59` | Integration target, not a parity GO |
| [NAPI](https://github.com/wasmerio/napi/tree/wasix/parity-unified) | `bdc247406e9cecd46888ce648a62d51ed2732378` | Native GC repair + relocated legacy dependency paths |
| [ABI/generator](https://github.com/wasix-org/wasixv2-abi/tree/c2c28b95e0c403b224f9cf83fa0e6d56a5a2f3a0) | `c2c28b95e0c403b224f9cf83fa0e6d56a5a2f3a0` | Canonical integration source |
| [Guest tests](https://github.com/wasix-org/wasixv2-tests/tree/c2795a1d22e3d00e373c2b3cb31cc1bf57b4de07) | `c2795a1d22e3d00e373c2b3cb31cc1bf57b4de07` | Test source with runtime-path migration |
| [libc2](https://github.com/wasix-org/libc2/tree/snapshot/wasixv2-20261105) | `ce830d6e`, branch `snapshot/wasixv2-20261105` | Dynamic libc source based on `feaf3753`, ABI/test pins aligned; excludes unfinished context wrappers |
| [wasixcc](https://github.com/wasix-org/wasixcc/tree/snapshot/wasixv2-20261105) | `29aba5dee92f`, branch `snapshot/wasixv2-20261105` | Source PIC table-argument correction |

These are source snapshots, not rebuilt or replaced installed sysroots/toolchains.
Use exact revisions, not unrelated shared checkout heads. Historical source/build
pin mismatches remain below as validation work.

## Priority: contexts and closures

- [ ] Finish native create/switch/destroy with exact retained invocations on the same
  thread/Store/memory; prove non-LIFO cycles, destruction, trap/exit/exec, signal/CPU
  scopes, thread-local environments, setjmp, main-only fork/vfork and teardown.
- [ ] Resolve the escaped-native-frame cancellation proof without weakening it.
  At stop: Restore regression passes after parsing WAT. The other test used
  `new_typed_async`, retained an incompatible synchronous Store context and got
  Ready(error) instead of Pending. Changing to the existing `new_async` sibling is
  drafted but not verified. Keep all physical-frame/error/lock assertions.
- [ ] Fix/prove cleanup transfer during executor shutdown. The failing test's panic
  exposed `Driver::drop -> LoaderCleanup::spawn -> QueueHost::spawn_local` recursive
  resubmission while queued cleanup was synchronously dropped, causing stack
  overflow/SIGSEGV. A valid-fixture pass alone does not close this lifecycle issue.
  Preserve a deliberate supported host-panic/cleanup regression and Store reuse proof.
- [ ] Complete closure allocation/prepare/reprepare/free through actual mm-wide
  publication, rollback, peer/future-store replay, mutable signature/cacheability,
  exception-safe shadow-stack restore, cancellation and original-error custody.
- [ ] Validate browser JSPI contexts' physical Promise ownership, callback handoff,
  canceled/abandoned waiters, exact typed/foreign errors, nested transitions and
  retirement. Browser source remains uncompiled at this checkpoint. Run real
  Chromium/Firefox worker tests and preserve explicit no-JSPI NOTSUP behavior.
- [ ] Run original linked v1 C consumers and separately labelled rebuilt-v2 consumers:
  context setjmp/process switching, closure lifetimes, buffer modes and guest exits.
  Preserve original producer flags; retain the pre-existing flaky
  `fork_after_switching` (#6538) status. Object/header checks are not execution proof.
- [ ] Close general JS `call_dynamic` exact-bit packet support: V128 currently returns
  EOPNOTSUPP; arbitrary floating NaN payload preservation through JS is unproved.
  Typed Wasm packet marshalling must preserve target identity, nested invocation
  storage, strict/partial/tail rules, async/error behavior and actual producer profiles.
- [ ] Regenerate/check canonical outputs, align generator/runtime/libc pins, finish
  strict affected lint and full relevant regression gates before integration.

Preceding native bounded results: exec contexts 5, v1 contexts 2, closure integration
6, strict Store-loan 1, generated equality 4, wait 34, syscalls 28 and kernel entry 3
passed. Subsequent generator units 44, C output comparison, affected native strict
Clippy and CLI build passed. This does not cover the failing physical proof or browser.

### Context/closure work branches (unfinished)

| Repository and branch | Commit | State |
| --- | --- | --- |
| [Runtime native](https://github.com/wasmerio/wasmer/tree/wasixv2/parity-contexts-closures) | `78cfe84b8b12c033a375c88902fd5769d1d1ac09` | Latest fixture changes captured; physical cleanup proof unresolved |
| [Runtime browser](https://github.com/wasmerio/wasmer/tree/wasixv2/parity-contexts-browser) | `c1711f2eeec14b1dd827c76bbe209fd4e800fcf1` | Native draft plus uncompiled JSPI ownership/error changes |
| [Generator](https://github.com/wasix-org/wasixv2-abi/tree/wip/context-closures-stop-20261105) | `2e57ab00aec6aef0a95df339f2bbc244f11aae7d` | Exact generator pin on both runtime drafts |
| [libc wrappers](https://github.com/wasix-org/libc2/tree/wasixv2/parity-contexts-closures) | `71ccfb3879d9eae8984f917219d703c70e285789` | Header/profile/object/install checks passed; runtime/link acceptance pending |

## Mandatory NAPI/custom bindings

- [ ] Finish optional v2 facade activation and protected leased refill. First facade
  compile found a non-Sync FunctionEnv handle; the reviewed short Mutex clone fix
  is captured, but the actual facade fixture has not passed yet.
- [ ] Prove allocator Ready/Pending/drop without legacy raw-grow fallback; exact
  backing/lease identity, mm-op blocking, per-Store shared-arena routing, managed
  cancellation/error preservation and closed/expired cleanup custody.
- [ ] Exercise real v2 custom native/JS bindings, DSO imports, finalizers and reentry,
  memory growth, retirement and supported memory/profile combinations. Do not infer
  private-memory or arbitrary-platform support from the shared-memory native profile.
- [ ] Integrate the facade only after these proofs. Native GC/frame repair is already
  integrated; its 80 NAPI / 39 callback tests and physical GC proofs do not prove
  facade activation or universal V8 package/platform compatibility.

Drafts: [runtime `wasixv2/parity-napi-facade-20261104`](https://github.com/wasmerio/wasmer/tree/wasixv2/parity-napi-facade-20261104)
`28dcdacf836f9786b0de323fd7ab029b844daecd`, paired with
[NAPI `parity/native-facade-20261104`](https://github.com/wasmerio/napi/tree/parity/native-facade-20261104)
`1ac102cde91dec6710865f22037ac44fccc862fd`.

## Mandatory SDK and remaining v1 consumers

- [ ] Deliver current and classic SDK public APIs through v2: raw/WEBC/memory package
  acquisition, command discovery/defaults/binary(), registry/cache, initial/package/
  exec loading, live directories/mounts, stdio/TTY, signals, kill and sibling isolation.
- [ ] Complete WISP/Ports/service-worker, Node and native Rust SDK integration.
- [ ] Prove actual source-built browser facade behavior under saturated queues,
  provisional launch/close, unread/blocked output, repeated waits, cancellation,
  process/guest PID distinctions, fatal worker failures and genuine zero-worker disposal.
- [ ] Resolve busy opaque `WebAssembly.Module` kill/isolation without rejecting old
  supported inputs, adding provenance flags, or fabricating completed termination.
- [ ] Finish package/public-Core source beyond the opt-in raw transport foundation.
  The old SDK remains default. TS, wasm metadata and strict Rust library lint passed;
  real browser execution and full SDK feature acceptance have not run.
- [ ] Port CLI package/cache/loading/HTTP helpers and C API WasiEnv/InstantiationHook/
  State consumers, Swift/root examples, sys-utils and old optional NAPI consumers
  before deleting their old runtime dependencies.
- [ ] Map and verify every retained feature before deleting legacy runtime/support
  crates (`wasmer-wasix`, wasi-types, virtual-fs/net/io/mio, journal and related users).
  Do not delete a shared provider merely because new guest execution uses v2.
  Keep legacy fixtures and old/new behavior evidence.

SDK draft: [wasmer-sdk `codex/sdk-v2-browser-core`](https://github.com/wasmerio/wasmer-sdk/tree/codex/sdk-v2-browser-core)
`32f497ec6d238c6e44e898a28442bc5b87643615`. Its exact runtime source pin remains
`25e60218af5d165727f7cdb06439fd61464ef8e4` deliberately; do not silently substitute
new directory layouts into its old-path build recipe.

Additional source-only package candidates recovered from the RAM staging area:
[SDK `wip/public-core-stop-20261105`](https://github.com/wasmerio/wasmer-sdk/tree/wip/public-core-stop-20261105)
`2a1382f163316097bebac67b9c7dd20a47f685b6`, paired with
[runtime `wip/sdk-package-seams-stop-20261105`](https://github.com/wasmerio/wasmer/tree/wip/sdk-package-seams-stop-20261105)
`50804206f722236533d3208cbad9b17ec1d46b74`. These add mixed-package/install/exec-file
seams but remain uncompiled; candidate lock refresh, lifecycle/code review, output
ownership/cancellation contracts and actual public execution are still required.
They are not part of the checked raw SDK foundation or integration snapshot.

## Other required behavior and integration

- [ ] Configured-provider bridge/unbridge/DHCP acquisition and route lifetimes.
  Network access-policy rules are delivered; those do not provide these controls.
  Use proper optional provider contracts and preserve authorization/error ordering.
- [ ] Correct coredump original-source function indices and instruction offsets
  through dynamic preparation and preemption before delivering trap dumps. Current
  observer/flag tests pass, but frames currently describe transformed code while
  naming the original input. Preserve mapping composition/cache/exec semantics;
  do not subtract a fixed import count or disable preemption.
- [ ] Recheck function/reflection table-mutation/cacheability, original-error,
  signature and custom-layer behavior with completed closures and contexts.
- [ ] Finish Chromium retirement diagnosis: debugger-free controls established
  sleeping worker OS threads persisting at 5 seconds; Firefox passed. Heap/context
  retention versus caching is unproved. Do not waive it as a known leak or claim
  that CDP target disappearance alone proves full resource reclamation.
- [ ] Reconcile V16's passed two-hour functional soak with residual RSS growth and
  finalize its handoff; no additional V16 production fix is integrated from that audit.

Coredump draft: [runtime `wasixv2/parity-coredump`](https://github.com/wasmerio/wasmer/tree/wasixv2/parity-coredump)
`5ffa51e4cf8e49c2480d2b2fa4b18e4dabf13209` (unmerged).
Additional reference-lifetime test draft:
[`wasixv2/parity-native-retirement`](https://github.com/wasmerio/wasmer/tree/wasixv2/parity-native-retirement)
`ed86c82897ff03ef202aed0c6dbaddbd250ef220`; do not infer it is integrated or newly validated.

## Acceptance, build and source reproducibility

- [ ] Run combined native suites, release CLI, actual old/new guest consumers and
  browser matrix on the exact integrated source tuple; enable/verify full native
  `make test-wasixv2` CI before a shipping claim. Feature composition and package
  identity must match production (including optional NAPI and browser no-defaults).
- [ ] Build private matched EH/PIC/asyncify C/C++ sysroots when resuming validation.
  Required LLVM runtime source: `ca7933e47d3a3451d81e72ac174dcb5aa28b59d1`;
  required Linux UAPI: `8d3ae59288f1e7d58d76558a6ee96d533bc5019f`.
  Historical shared rebuild used Linux `d135ab45`, so its success is not exact-pin proof.
  Do not overwrite shared installed artifacts without separate authorization.
- [ ] Preserve eight inherited Chromium cleanup failures, expanded wasm all-target
  native-only-test import failures, Node-only `Worker` absence and existing compiler
  lint/ignored external-fixture limits until specifically resolved or scoped with evidence.
- [ ] Supply `WASIXV2_TOOLCHAIN_TOKEN` with required producer-repo access before rerunning
  the blocked B17 source-producer CI; adapter/runtime green did not make skipped fixtures pass.
- [ ] Finish strict Rust/JS/format gates. Aggregate `make lint` currently stops at
  missing `yamlfmt`; record this tooling gap rather than claim the aggregate passed.
- [ ] Validate renamed paths/scripts/submodules across downstream repos; use one source
  identity for each core crate, not mixed path/git/registry duplicates.
- [ ] Keep disk/build budgets explicit, reuse only compatible artifacts, and verify
  actual own-source compilation. Prior cross-worktree warm-cache reuse was misleading.

## Intentional deferrals / later design work

- Snapshot/restore: intentionally excluded; unused legacy feature to be redesigned.
- Windows and nominal WASIX64: deferred by the user; not current parity blockers.
- RTLD_NEXT: deferred. Full Linux compatibility is not the completion criterion.
- Full-image dynamic copying fork / multi-module asyncify, TLS-dependent initializer
  snapshots and unsupported dynamic relocation forms require their own consumer-backed
  contracts; do not silently enable or claim them. Audit actual old supported profiles
  before treating a new mode as mandatory parity.
- Record/replay, dedicated embedder-policy implementation, paused memory-accounting
  redesign and host-enforced page-protection/userfaultfd extensions remain future/design
  work beyond delivered per-feature controls; no blanket implementation claim.
- Separate pure kernel repository: evaluation says the 21 engine-free crates can move,
  leaving api/exec/dylink/wasix1 in main. No extraction approved or implemented. Before
  doing it, independently review the cut, shared generator, test ownership, source pins
  and cross-repo CI. Do not use extraction to defer current compatibility delivery.

Deferred source is preserved in
[runtime `wasixv2/parity-wasix64`](https://github.com/wasmerio/wasmer/tree/wasixv2/parity-wasix64)
`4f0672abf00cf9744976ad3687d32ebbb1f9dfeb`, paired with
[generator `wip/wasix64-stop-20261105`](https://github.com/wasix-org/wasixv2-abi/tree/wip/wasix64-stop-20261105)
`62378c78aa6a04daaae69ddba02ad2b9daff742e`. This draft is uncompiled/unregenerated,
not a supported Memory64 implementation, and must not be merged just to clear TODOs.
