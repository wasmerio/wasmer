# WASIX v2 dynamic linking

`Module::parse` and `Plan::new` implement the metadata/planning part of
[DESIGN.md](../DESIGN.md#dynamic-linking). The native runtime connects the four existing `dl_*` imports to per-image
source graphs and per-store instances for the bounded native profile below.
The native staging component executes no guest initialization helpers; libc2
calls constructors after publication.

The caller supplies validated module descriptions, identity-deduplicated dependency
edges matching each `needed` list, the root and the global lookup order. The runtime opens
files through the current caller's guest namespace and retains the opened Image
backing for identity lifetime. The pure planner preserves runtime paths; the
native runtime searches literal paths without `$ORIGIN` or other expansion.

Supported metadata is wasm32 core modules with function signatures, import/export
indices and types, `dylink.0` allocation information, dependencies, runtime paths
and symbol flags. Duplicate sections/entries, dangling strong flag entries, malformed or
trailing subsection fields, invalid alignments and unknown flag bits fail parsing.
Undefined weak annotations omitted from actual wasm imports by lld are retained;
semantic `env` flags also apply to GOT imports. Unknown subsections are retained
for a later compatibility check, not interpreted.
GC/typed-function signatures, exact function imports/exports and wasm64 are rejected.

Plans contain cycle-safe graph order, aligned relative memory/table reservations,
direct function and GOT fixups, and separate TLS/absolute/data offsets. Tables
reserve null at zero and the existing signal sentinels at one and two. Global scope precedes the local load group; hidden/local
definitions bind locally. Lookup takes the first visible definition including weak
definitions, as musl's `ldso/dynlink.c:find_sym2` does. Missing strong NOW references
fail, missing weak references become zero, and unresolved LAZY references remain
explicitly deferred. Type errors are errors in either mode. Non-linker import
namespaces and well-known `env` infrastructure are host bindings requiring checks.
This is userspace linker policy; Linux `fs/binfmt_elf.c:load_elf_binary` loads the
interpreter, rather than implementing these symbol lookups in the kernel.

Reservations describe a fresh relative layout, not additions to an existing live
image. The image owner must supply aligned bases and reconcile proposed function
slots with element segments and imported function aliases. `deferred_checks`
records that reconciliation, host import checking and unknown global values.

`Initialization::parse` validates finite helper bodies and the exact supported lld
shared-memory once template, then produces fresh-range-checked host writes, scalar
global updates and element placements. It never executes wasm; helper names alone
confer no trust. It rejects imported calls, constructors, recursion, loops and writes
to imported globals. The no-auto-start rewrite makes active data empty passive
segments and active elements declared while preserving segment indices. Private
exports expose function/global/memory/tag identity for later engine preparation.
TLS pointer relocations are applied after each thread's final TLS global fixups.
`prepare_image` explicitly prepares a native dynamic main and retains its original
validated source in a reserved custom section; it rejects startup dependencies and
asyncify. The native facade
compiler uses it for initial images and exec; custom loaders remain explicit.
The runtime verifies the deterministic sanitized/instrumented source against the
native compiled module hash before host initialization.

The native path supports NOW explicit handles, exported data/functions, per-store
DSO TLS, cyclic direct-function dependencies with resolved NOW targets, existing
and future peers, shared `Image` CLONE_VM groups, missing-symbol rollback, and
no-unload close. Existing admission registers every store; an atomic
mm image-owner token rejects mixed/custom or independently selected owners.
All registered stores park, prepare and apply validated host effects before atomic
graph publication; libc2 drains constructors afterward. Fresh exec selects a new
image/mm, retaining surviving old-mm attachments. Shared graphs contain source,
layout and file owners, never store-local engine objects.

When a resolved direct-function provider has not yet been instantiated, the store
creates a typed Wasm forwarding function calling its reserved table slot. These
functions use normal exec instrumentation, with no symbol lookup or per-call host
resolver. Their instances remain stage-owned until publication and are retained
with the committed per-store graph afterward. Imported/re-exported function
chains resolve to concrete definitions; table elements and symbol addresses use
those definitions rather than forwarding aliases. All targets are filled by
host Apply before atomic publication, and rollback clears only new slots. Future
stores replay the same frozen NOW targets with their own forwarding instances.

The profile fails closed for startup dependencies, asyncify, LAZY/NEXT,
caller-sensitive lookup, unsupported initializer bodies, definition-free function
alias cycles and main-TLS crossimports that need post-libc late binding. DSO-local TLS
and calls through main libc TLS accessors are supported. The real C source proof
uses a three-argument main; it does not establish general C/C++ program acceptance.
Ptrace syscall-entry/exit stops retain their exact pending future. An opt-in per-store
driver pauses that future only at a kernel-origin syscall-entry/exit stop, services
validated host loader work under the store lock, then resumes the same future without
continuing the tracee or replaying ptrace events. Other pending kernel operations
keep progressing to their ordinary safe return boundary; they may hold loader-needed
locks. Mutable tracer siginfo cannot grant outside cooperation. Default/custom ABIs
remain opted out; no acknowledgement is fabricated.
No full browser/fork/asyncify acceptance is claimed.

Managed runners await store retirement and drain only their counted loader cleanup
jobs; unrelated host work remains detached. Weak final callbacks run after guest/task
teardown and prior loader jobs. Kernel membership by original gate includes unstarted
and surviving CLONE_VM users; common mappings retire only after the last user, while
retained Rust handles keep metadata and the owner nonce until their final drop.
Retired images/stores reject entry and loader reuse. Exec keeps a fresh mm independent
from surviving old-mm users, and dead weak callbacks are pruned across replacements.

Cancellation transfers the exact suspended cleanup continuation; provided native
workers finish their loader scope before exiting. Externally driven callers must
keep the executor context alive for transferred cleanup, as required by the host
executor. This preserves the existing whole-run Kernel cancellation policy and does
not provide recovery after the embedding application shuts that executor down.

The pure core tests do not need the engine:

```sh
cargo test -p wasixv2-dylink --no-default-features
```

The existing default `sys` and optional `js` engine feature wiring is retained.

With `sys`, `StagedModule` implements one store's prepare/apply/rollback/publication
building block. Preparation validates and rewrites source itself, checks the supplied
address-space memory/table identity, preflights globals and fresh ranges, reconciles
active element and planned function identities, and reserves table capacity. Side
code receives the existing exec preemption/atomic/growth instrumentation, failing
closed when that rewrite is unsupported; imports must supply exec's normal host
hooks. This preserves the main image's return-to-guest work boundary in side code. Apply
uses only cached host writes/global/table assignments, with no awaits or guest calls.
Rollback clears staged bytes and slots; old image state and signal sentinel slots
remain untouched. Publication consumes an applied staging object. The caller must
retain the object for rollback until the mm-wide transaction publishes every store;
there is no implicit cross-store coordination or cancellation guarantee in this type.
The runtime coordinator supplies mm-wide phases, image ownership and the libc
protocol; `StagedModule` alone does not guarantee their lifecycle. Wasm physical capacity growth cannot be rolled back.

The native engine unit proof is explicitly opt-in so pure planner tests remain cheap:

```sh
cargo test -p wasixv2-dylink --no-default-features --features engine-tests
```
