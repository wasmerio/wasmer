# WASIX v2 link planning

`Module::parse` and `Plan::new` implement the metadata/planning part of
[DESIGN.md](../DESIGN.md#dynamic-linking). This is not a working dynamic loader:
the four `dl_*` imports remain unsupported until the process-level integration is installed.
The optional native staging component compiles and instantiates validated no-start
modules, but executes no guest initialization or constructor code.

The caller supplies validated module descriptions, identity-deduplicated dependency
edges matching each `needed` list, the root and the global lookup order. Opening
files in the guest namespace remains the future host/libc2 loader's responsibility.
The planner preserves runtime paths without expanding or searching them.

Supported metadata is wasm32 core modules with function signatures, import/export
indices and types, `dylink.0` allocation information, dependencies, runtime paths
and symbol flags. Duplicate sections/entries, dangling flag entries, malformed or
trailing subsection fields, invalid alignments and unknown flag bits fail parsing.
Unknown subsections are retained for a later compatibility check, not interpreted.
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
asyncify. It is not connected to the exec compiler or runtime yet.

Live instantiation, rollback/publication, thread rendezvous, CLONE_VM ownership,
caller-sensitive lookup, startup loading, libc2 wrappers, PIC toolchain enablement
and cross-module fork/exec require the remaining approved integration work. The
four imports still have their existing unsupported runtime behavior. The source
fixtures do not prove cross-module asyncify or caller-identity support.

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
The mm coordinator, image ownership factory and libc protocol are still required
before runtime `dlopen` works. Wasm physical capacity growth cannot be rolled back.

The native engine unit proof is explicitly opt-in so pure planner tests remain cheap:

```sh
cargo test -p wasixv2-dylink --no-default-features --features engine-tests
```
