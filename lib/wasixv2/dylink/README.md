# WASIX v2 link planning

`Module::parse` and `Plan::new` implement the metadata/planning part of
[DESIGN.md](../DESIGN.md#dynamic-linking). This is not a working dynamic loader:
no guest code is compiled or run and the four `dl_*` imports remain unchanged.

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
Initializers (including start functions) are never executed, and names such as
`__wasm_init_tls` confer no trust. The source fixtures exercise only those metadata
boundaries; they do not prove cross-module asyncify or caller-identity support.

Live instantiation, initialization validation, rollback/publication, thread
rendezvous, CLONE_VM ownership, caller-sensitive lookup, startup loading, libc2
wrappers, PIC toolchain enablement and cross-module fork/exec require subsequent
approved integration work. No ABI or lifecycle contract is introduced here.

The pure core tests do not need the engine:

```sh
cargo test -p wasixv2-dylink --no-default-features
```

The existing default `sys` and optional `js` engine feature wiring is retained.
