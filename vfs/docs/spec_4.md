# Step 4 Spec: Sync + Async Interface Design (Phase 4)

This document is the **detailed implementation spec** for **Phase 4** from `fs-refactor.md`:

- **4.1 Sync trait API**
- **4.2 Async trait API**
- **4.3 Adapters and ergonomics**
- **Phase 4 acceptance criteria**

It is written to be executable by a junior engineer and also to make explicit how Phase 4
interfaces impact and are used by Phases 1–3 (core semantics + mount/path walker + overlay) and
enable Phase 5 (Wasix integration).

---

## Goal (Phase 4 in one sentence)

Define an **object-safe, performance-aware sync + async trait surface** for the VFS backend layer
and provide **bridges/adapters** so that a backend implementing **only sync** or **only async**
can still be used from both sync and async VFS entry points.

---

## Why Phase 4 matters to the rest of the plan

Phase 4 is a “cross-cutting interface phase”. If it’s wrong, later phases either:

- duplicate logic (sync vs async codepaths diverge),
- block incorrectly (async-only backends become unusable), or
- regress correctness/performance (extra allocations/locks on hot paths).

Phase relationships:

- **Phase 1 (boundaries + layering contract)**: Phase 4 must preserve the rule that `vfs-core`
  is the **semantic source of truth** and that runtime coupling (tokio/etc.) is not hard-coded.
- **Phase 2 (core semantics)**: Phase 4 must support “at”-style semantics, `PathWalker` rules,
  and OFD (open file description) semantics in both sync and async usage.
- **Phase 3 (mounts + overlay)**: mounts and overlay are **composition points**. The trait design
  must allow a mount table entry to provide sync and/or async operations for the mounted filesystem,
  and overlay must be able to delegate to lower/upper layers in whichever mode it is called.
- **Phase 5 (Wasix integration)**: Wasix must be able to call into a **stable VFS API** from both
  sync and async contexts (depending on how the runtime and syscalls are structured). Phase 4 defines
  the backend traits and the runtime hooks that Phase 5 relies on.

---

## Current code status (important: alignment vs required changes)

There is already non-trivial implementation work in this repository. Phase 4 must **build on it**
instead of fighting it.

### What already aligns well

- **Async uses `async-trait`** and is object-safe:
  - `vfs/core/src/node.rs` defines `FsNodeAsync` and `FsHandleAsync` with `#[async_trait]`.
- **Sync traits exist** for node and handle:
  - `vfs/core/src/node.rs` defines `FsNode` and `FsHandle` (sync), plus marker traits `FsNodeSync`
    and `FsHandleSync`.
- **OFD semantics implemented for sync**:
  - `vfs/core/src/handle.rs` implements `VfsHandle` with correct shared-offset behavior (a key POSIX
    requirement from the plan’s layering contract).
- **Provider registry exists (sync)**:
  - `vfs/core/src/provider_registry.rs` stores `Arc<dyn FsProvider>` and mounts sync `Fs` instances.
- **Mount table exists (sync)** and matches the plan’s inode-driven mount transitions:
  - `vfs/core/src/mount.rs` stores `Arc<dyn Fs>` and maps mountpoints by inode.
- **PathWalker exists (sync)** and is mount-aware:
  - `vfs/core/src/path_walker.rs` traverses using `FsNode` methods (sync).

### What is missing for Phase 4 completion

Phase 4 requires **end-to-end** sync+async interfaces, not only node/handle traits:

- **Missing async equivalents** for:
  - `Fs` (filesystem instance / superblock-like)
  - `FsProvider` (mounting interface)
  - mount table accessors that need `node_by_inode` in async mode (used for `..` at mount root)
  - path walking (needs `lookup/readlink/metadata` potentially async)
  - VFS-level handle type with OFD semantics for async backends
- **Adapters exist in name only**:
  - `vfs/core/src/provider.rs` contains `VfsRuntime`, `AsyncAdapter<T>`, `SyncAdapter<T>`, but there
    are no concrete adapter implementations that “lift” a sync backend into async traits and vice-versa.
- **Runtime crate exists but is a stub**:
  - `vfs/rt` exists but does not yet provide runtime hooks used by adapters.

### Constraints implied by existing code

- The codebase already picked `async-trait` for async node/handle traits. Phase 4 should **standardize**
  that choice across all async backend traits (`FsAsync`, `FsProviderAsync`), because mixing patterns
  increases friction and leads to inconsistent dynamic-dispatch behavior.
- The current OFD implementation (`VfsHandle`) uses a **blocking mutex** (`parking_lot::Mutex`) and
  holds it across backend I/O. That’s correct for sync. For async, we must not hold a blocking mutex
  across an `.await`. Therefore we need a separate async OFD lock strategy.

---

## Design principles (Phase 4 “rules of the road”)

### 1) Object safety and dynamic dispatch

The provider registry must store heterogeneous providers as trait objects (`dyn`). Therefore:

- all “async traits” must be usable as `dyn Trait` (object-safe).
- we will use `async-trait` for async traits (as the plan states and as current code already does).

### 2) Backends should not duplicate semantics

Backends implement primitive operations; `vfs-core` applies POSIX-ish semantics:

- Path traversal and mount semantics: `PathWalker` / `PathWalkerAsync`.
- OFD semantics: `VfsHandle` / `VfsHandleAsync`.
- Policy checks: `VfsPolicy` (already in core) must be callable from both sync and async operations.

### 3) “Zero-cost when unused”

When an application uses only sync APIs:

- there should be no mandatory async runtime dependency.
- there should be no per-operation async allocation.

When using async APIs:

- overhead is allowed but must be bounded and predictable.

### 4) Explicit runtime hooks (no tokio in `vfs-core`)

Adapters need:

- **spawn-blocking** to expose a sync backend via async APIs
- **block-on** to expose an async backend via sync APIs

These are expressed via a runtime hook trait that is runtime-agnostic. Implementations live in `vfs-rt`
and/or `lib/wasix` depending on integration choice.

### 5) Correctness for async backends is non-negotiable

We must support backends that are async-native (network/object store). That implies:

- Async path walking must exist (cannot just spawn-blocking the entire syscall).
- Async OFD semantics must exist (cannot use blocking mutex + `.await`).

---

## Phase 4 deliverables (what files/modules must exist after this phase)

Phase 4 deliverables as specified in the plan:

- `vfs/core/src/traits_sync.rs`
- `vfs/core/src/traits_async.rs`
- Runtime bridging hooks in `vfs/rt` (crate: `vfs-rt`)

**Important note about existing code**:

The repository currently defines several traits in `vfs/core/src/node.rs`, `vfs/core/src/fs.rs`,
and `vfs/core/src/provider.rs`. Phase 4 should:

- **move or re-export** those traits into the new trait modules,
- keep public API stability where feasible (re-exports are fine),
- avoid huge single files (“no huge files” constraint in the plan).

Recommended final module layout (concrete):

- `vfs/core/src/traits_sync.rs`
  - `FsProviderSync`, `FsSync`, `FsNodeSync`, `FsHandleSync`
  - sync-only helper types for trait signatures (if any)
- `vfs/core/src/traits_async.rs`
  - `FsProviderAsync`, `FsAsync`, `FsNodeAsync`, `FsHandleAsync`
  - `#[async_trait]` usage, `Send + Sync` bounds
- `vfs/core/src/node.rs`
  - keep **types** and small helpers (options structs), but re-export traits from `traits_*`
  - or split `node.rs` into `node/mod.rs` + `node/types.rs` if it grows
- `vfs/core/src/fs.rs`, `vfs/core/src/provider.rs`
  - become thin wrappers or are subsumed by `traits_sync.rs`/`traits_async.rs`
  - keep capability flags and config types where they are today (provider capabilities already exist)

---

## 4.1 Sync trait API (detailed spec)

### 4.1.1 Definitions

#### `FsProviderSync`

Role: registry-visible “filesystem type/driver” that can create filesystem instances.

Required methods:

- `fn name(&self) -> &'static str`
- `fn capabilities(&self) -> FsProviderCapabilities`
- `fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()>` (default `Ok(())`)
- `fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn FsSync>>`

Notes:

- This is effectively today’s `FsProvider` trait, but its return type must be `FsSync`.
- Keep `ProviderConfig` type erasure and downcast helpers (already implemented).

#### `FsSync`

Role: a mounted filesystem instance (“superblock-like”).

Required methods:

- `fn provider_name(&self) -> &'static str`
- `fn capabilities(&self) -> VfsCapabilities`
- `fn root(&self) -> Arc<dyn FsNodeSync>`

Optional method:

- `fn node_by_inode(&self, inode: BackendInodeId) -> Option<Arc<dyn FsNodeSync>>`
  - needed for mount-root `..` behavior in the current sync `PathWalker`
  - if a backend cannot support this, `PathWalker` must still behave correctly where possible:
    - mount-root `..` resolution may fail with `VfsErrorKind::Stale` or `NotSupported` depending on
      which invariant is broken (see “Error contracts” below).

Alignment with existing code:

- `vfs/core/src/fs.rs` already defines `Fs` with the correct shape (sync).
- `vfs/mem` and `vfs/overlay` already implement `Fs` (sync).
- Phase 4 change is primarily **renaming / trait re-exporting** and ensuring the type names align
  with the plan.

#### `FsNodeSync` and `FsHandleSync`

These correspond to the existing sync traits:

- `FsNode` (sync): lookup/create/mkdir/unlink/rmdir/read_dir/rename/link/symlink/readlink/open
- `FsHandle` (sync): read_at/write_at/flush/fsync/len/set_len/dup/is_seekable

Phase 4 must:

- standardize on the naming `FsNodeSync` / `FsHandleSync` as the “official” sync traits in
  `traits_sync.rs`,
- keep `FsNode`/`FsHandle` as type aliases or re-exports if they are already used widely.

### 4.1.2 Sync trait error contracts

All traits must return `VfsResult<T>` and use `VfsErrorKind` consistently.

Rules:

- `NotFound` for missing names during lookup/unlink/etc.
- `NotDir` when a path component is not a directory but must be.
- `IsDir` when attempting a file op on a directory (e.g. open regular-file handle).
- `NotSupported` for unsupported operations *as declared by capabilities*.
- `CrossDevice` for:
  - operations across mounts (handled in VFS core, not backend),
  - attempts to link/rename across different filesystem instances (backend may detect via downcast
    mismatch and return `CrossDevice`).

### 4.1.3 Sync-only performance requirements

- Sync trait methods must be “hot-path friendly”:
  - avoid allocation-heavy return types in hot ops.
  - prefer `SmallVec` (already used in `ReadDirBatch`) for directory enumeration batches.
- Avoid per-call string formatting in errors; prefer static contexts (`&'static str`) as done in
  `VfsError::new`.

---

## 4.2 Async trait API (detailed spec)

### 4.2.1 Definitions

Async traits mirror sync traits but are **async-native** and object-safe via `async-trait`.

#### `FsProviderAsync`

Role: async-native filesystem provider.

Required methods:

- `fn name(&self) -> &'static str`
- `fn capabilities(&self) -> FsProviderCapabilities`
- `fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()>` (sync, default `Ok(())`)
- `async fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn FsAsync>>`

Notes:

- `validate_config` remains sync because it should be CPU-only and cheap.
- `mount` is async because backends may need I/O to mount (auth, network, metadata fetch).

#### `FsAsync`

Role: async filesystem instance.

Required methods:

- `fn provider_name(&self) -> &'static str`
- `fn capabilities(&self) -> VfsCapabilities`
- `async fn root(&self) -> VfsResult<Arc<dyn FsNodeAsync>>`

Optional methods:

- `async fn node_by_inode(&self, inode: BackendInodeId) -> VfsResult<Option<Arc<dyn FsNodeAsync>>>`
  - used for mount-root `..` resolution in async path walking and detached-mount handle traversal.

Why `root()` is async:

- For some backends, opening the root handle may require I/O or lazy initialization.
- If a backend can return root cheaply, it can do so without awaiting.

#### `FsNodeAsync` and `FsHandleAsync`

These already exist in `vfs/core/src/node.rs` today and should be migrated/re-exported.

Phase 4 must ensure:

- all async trait methods are `Send` where needed (see next section),
- returned trait objects are `Arc<dyn ...>` and are safe to use across tasks/threads.

### 4.2.2 Send + Sync requirements (explicit)

For registry and mount table storage, we need thread-safe sharing:

- `FsProviderAsync: Send + Sync`
- `FsAsync: Send + Sync`
- `FsNodeAsync: Send + Sync`
- `FsHandleAsync: Send + Sync`

For returned futures from `async-trait`:

- require `Send` futures (default `async-trait` behavior if the trait itself is `Send + Sync` and
  methods use `&self`).

### 4.2.3 Async error contracts

Same as sync: `VfsErrorKind` must remain stable and meaningful. Async implementations should not
leak runtime-specific cancellation errors; map those to:

- `Cancelled` (preferred) or `Interrupted` depending on semantics.

---

## 4.3 Adapters and ergonomics (detailed spec)

Phase 4 requires bidirectional adapters:

- **Sync backend usable from async API**
- **Async backend usable from sync API**

This section defines adapter behavior, where they live, and how they compose with the mount table,
path walker, and OFD semantics.

### 4.3.1 Runtime hook contract (`vfs-rt`)

Adapters need runtime services but `vfs-core` must not depend on tokio directly.

Define a runtime hook trait that can be implemented by `vfs-rt`:

- `spawn_blocking`: run blocking sync code off-thread and await it
- `block_on`: run an async future to completion (used only when exposing async backends as sync)

Status today:

- A trait named `VfsRuntime` already exists in `vfs/core/src/provider.rs` with these methods.

Phase 4 decision:

- Keep the trait definition in `vfs-core` (it is runtime-agnostic and does not create a tokio dep).
- Implement concrete runtimes in `vfs-rt` (e.g. tokio-backed, async-std-backed, and a test runtime).
- Update docs and exports so downstream users use `vfs_rt::*` implementations.

Concrete deliverables in `vfs/rt`:

- `TokioRuntime` (feature-gated behind `tokio` if needed by workspace policy)
- `InlineTestRuntime` for unit tests (uses a simple executor for `block_on` and a threadpool for
  `spawn_blocking`, or a minimal “spawn a thread per blocking task” approach for tests only)

### 4.3.2 Adapter strategy (what gets adapted)

Adapters must adapt the entire backend stack, not only providers:

- Provider → Fs → Node → Handle

If we only adapt the provider mount call, but return an `Fs` that returns the “wrong” node type,
the async VFS code cannot operate.

Therefore each adapter is a family of wrapper structs, typically:

- `AsyncProviderFromSync`
- `AsyncFsFromSync`
- `AsyncNodeFromSync`
- `AsyncHandleFromSync`

and the reverse:

- `SyncProviderFromAsync`
- `SyncFsFromAsync`
- `SyncNodeFromAsync`
- `SyncHandleFromAsync`

### 4.3.3 `Async*FromSync` adapter (sync → async)

Use `spawn_blocking` per operation.

Rules:

- The wrapper must clone the inner `Arc<dyn SyncTrait>` as needed.
- Each async method:
  - clones arguments as required (avoid capturing references that are not `'static` unless they
    can be copied into owned buffers),
  - calls `rt.spawn_blocking(move || inner.sync_method(...))`,
  - awaits and returns the result.

Performance guidance:

- For high-frequency operations (e.g. small reads), doing a spawn-blocking per call can be expensive.
  This is acceptable for Phase 4 correctness, but later phases may optimize by:
  - batching,
  - using async-native backends,
  - or adding specialized “blocking I/O pool” semantics.

### 4.3.4 `Sync*FromAsync` adapter (async → sync)

Use `block_on` per operation.

Rules:

- This adapter should be used carefully to avoid deadlocks if called inside an async runtime thread.
- In VFS core, we treat this adapter as “allowed, but caller-controlled”:
  - Wasix and other callers must decide whether it is safe to call sync APIs in their environment.

Implementation rule:

- Each sync method calls `rt.block_on(inner.async_method(...))`.

### 4.3.5 Where adapters are applied (mount time is preferred)

Adapters should be applied as early as possible (at mount time), so the rest of the VFS core can
assume a consistent interface.

Recommended approach:

- Every `MountEntry` stores **both** interfaces:
  - `fs_sync: Arc<dyn FsSync>`
  - `fs_async: Arc<dyn FsAsync>`
- When mounting:
  - if the provider is sync-only, construct `fs_async` via `AsyncFsFromSync`
  - if the provider is async-only, construct `fs_sync` via `SyncFsFromAsync`
  - if the provider supports both, store both directly (no adapter cost).

This avoids maintaining two separate mount tables and keeps mount topology shared.

Required mount table changes:

- `vfs/core/src/mount.rs` must be updated so `MountEntry` stores both forms (or stores a unified
  “mount fs” wrapper that exposes both).

### 4.3.6 Async path walking (required for async-native backends)

Status today:

- `PathWalker` is sync-only and calls `FsNode` methods directly.

Phase 4 requirement:

- Provide an async equivalent: `PathWalkerAsync`.

Key design constraints:

- `PathWalkerAsync` must be mount-aware the same way as `PathWalker`.
- It must support the same flags and semantics:
  - symlink following vs nofollow
  - max symlink depth
  - trailing slash rules
  - `resolve_beneath` and `in_root` flags
  - mount boundary `..`
- It must not hold blocking locks across await.

Implementation outline (junior-friendly):

- Copy the structure of the existing sync walker and replace:
  - `node.lookup(...)` → `node.lookup(...).await`
  - `node.readlink()` → `node.readlink().await`
  - `node.metadata()` → `node.metadata().await`
  - mount-root parent resolution uses `fs_async.node_by_inode(...).await`
- Keep the mount table snapshot strategy:
  - take `Arc<MountTableInner>` snapshot once per resolve call and keep it across awaits (safe because
    it’s an `Arc` and immutable).

Required type additions:

- `NodeRefAsync` (analogous to `NodeRef`) holding:
  - `mount: MountId`
  - `node: Arc<dyn FsNodeAsync>`
- `VfsDirHandleAsync` (see below) to represent async base directories.

### 4.3.7 Async OFD semantics (required for correctness)

Status today:

- `VfsHandle` provides sync OFD semantics using a blocking mutex to serialize shared-offset I/O.

Phase 4 requirement:

- Provide `VfsHandleAsync` with equivalent semantics for async `FsHandleAsync`.

Core correctness requirement:

- Concurrent `read()`/`write()` on duplicated handles must behave as if operations are serialized
  with a shared offset (POSIX OFD semantics).

Implementation approach:

- Create a separate async OFD state type:
  - `backend: Arc<dyn FsHandleAsync>`
  - `offset: AtomicU64`
  - `status_flags: AtomicU32`
  - `io_lock: AsyncMutex<()>` (must be awaitable)

Dependency policy:

- `vfs-core` currently depends only on `async-trait`, `parking_lot`, `smallvec`, `bitflags`.
- For an async mutex, add a small runtime-agnostic crate dependency such as:
  - `async-lock` (recommended for simplicity), or
  - another minimal async mutex crate that does not force a specific runtime.

Rules for implementation:

- `read(buf).await`:
  - acquire `io_lock` (await)
  - read offset
  - `backend.read_at(offset, buf).await`
  - update offset
- `write(buf).await`:
  - same, but handle `APPEND`:
    - if `APPEND`, fetch size via `backend.get_metadata().await` (or a dedicated `len()` if present),
      then write at end and update offset.

### 4.3.8 Directory handles for async “at”-style operations

Status today:

- `VfsBaseDir` supports:
  - `Cwd`
  - `Handle(&VfsDirHandle)`
- `VfsDirHandle` holds a sync node `Arc<dyn FsNode>`.

Phase 4 requirement:

- Define async equivalents:
  - `VfsDirHandleAsync` holding `Arc<dyn FsNodeAsync>` plus parent `NodeRefAsync` (optional)
  - `VfsBaseDirAsync<'a>` analogous to `VfsBaseDir<'a>`

Rationale:

- Async path walking needs an async node as its starting point; it cannot call sync methods.

### 4.3.9 Provider registry ergonomics (sync + async aware)

Status today:

- `FsProviderRegistry` stores only sync `FsProvider`.

Phase 4 requirement:

- Extend the registry to support both:
  - `register_sync(provider: Arc<dyn FsProviderSync>)`
  - `register_async(provider: Arc<dyn FsProviderAsync>)`
  - `register(provider)` convenience for providers that implement both (optional)

Mounting APIs:

- `create_fs_sync(...) -> Arc<dyn FsSync>`
- `create_fs_async(...).await -> Arc<dyn FsAsync>`
- `mount_with_provider_*` should mirror current helpers but in both modes.

Adapter integration:

- The registry itself does not have to create both interfaces immediately, but the mount layer should
  (preferred). If the mount layer expects both, then registry mount should accept a runtime handle to
  construct adapters at mount time.

### 4.3.10 Overlay and composition in async mode

Status today:

- `vfs-overlay` is sync-only and delegates to sync `FsNode`/`FsHandle`.

Phase 4 requirement:

- Overlay must remain composable in both modes:
  - either implement native async overlay traits (later), or
  - rely on adapters so that a sync overlay can be used from async APIs.

Phase 4 decision (pragmatic):

- Do not require implementing async-native overlay in Phase 4.
- Require that adapters work well enough that:
  - a sync overlay filesystem mounted into the mount table produces a usable `fs_async` interface
    via `AsyncFsFromSync`.

---

## Integration points and ordering (how Phase 4 affects other modules)

### Path walker ↔ mount table

- The mount table must expose:
  - sync `root()` node access
  - async `root().await` node access
  - mount-root parent resolution in both modes (`node_by_inode` vs `node_by_inode().await`)

### VFS entry points (`vfs/core/src/vfs.rs`)

Status today:

- `Vfs` exists but is a stub and sync-only.

Phase 4 requirement:

- Add `VfsAsync` (or `impl Vfs { async fn ... }` if keeping one type):
  - `openat_async`, `statat_async`, `mkdirat_async`, `unlinkat_async`, `renameat_async`,
    `readlinkat_async`, `symlinkat_async`, `readdir_async` (mirrors sync API)

Rule:

- Sync VFS calls sync path walker and sync backend traits.
- Async VFS calls async path walker and async backend traits.

### Policy checks

Status today:

- `PathWalker` calls `ctx.policy.check_path_component_traverse(...)` after doing a sync metadata call.

Phase 4 requirement:

- Ensure policy checks can be performed in async path walking:
  - either keep policy checks sync (operating on metadata already fetched async),
  - or provide async policy hooks only if needed later.

Recommended:

- Keep policy checks sync; they are CPU-only and should not require awaiting.

### Rate limiting, quotas, watching (future phases)

Phase 4 must not block these features:

- Rate limiting (Phase 3.7 spec) requires hooks in both sync and async dispatch.
  - Ensure async VFS APIs can pass a “nonblocking” flag and surface `WouldBlock`.
- Quotas/limits: accounting may be in backend or VFS core; the trait design must allow returning
  `NoSpace` / `QuotaExceeded`.
- Watching: future watch traits must be defined in both sync and async forms.
  - Phase 4 naming (`*_Sync`, `*_Async`) should be applied consistently when that module is added.

---

## Phase 4 acceptance criteria (expanded into concrete tests)

The plan’s acceptance criteria:

> One backend can be implemented using only the sync traits and still be usable from the async
> interface (via adapters), and vice-versa. The chosen async-trait strategy is implemented and
> covered by at least one unit test exercising a sync<->async adapter boundary.

Concrete acceptance criteria for this repository:

### A) Sync-only backend usable from async API

- Use `vfs-mem` as the sync-only backend (it implements sync traits today).
- Mount it such that `fs_async` exists via `Async*FromSync` adapters.
- Add a test (location suggestion: `vfs/core/tests/phase4_adapters.rs`) that:
  - mounts memfs through an async mount path (or wraps a mounted sync fs into async),
  - performs:
    - `lookup` on the root
    - `readlink` on a symlink
    - `open` a file and `read_at`/`write_at`
  - verifies it works without panics and returns correct bytes.

### B) Async-only backend usable from sync API

- Create a minimal “fake async fs” in tests that implements:
  - `FsProviderAsync`, `FsAsync`, `FsNodeAsync`, `FsHandleAsync`
  - methods can be trivial and store state in `Arc<Mutex<...>>`
- Wrap it with `Sync*FromAsync` adapters using a test runtime’s `block_on`.
- Call a small sync operation:
  - `root().lookup(...)` via the sync wrapper
  - and ensure error kinds are propagated correctly.

### C) OFD semantics are preserved in async handle type

- Construct `VfsHandleAsync` from a handle that:
  - records offsets accessed
- `dup` a handle (or clone the VFS handle representing the same OFD)
- Run two concurrent async reads/writes and verify:
  - offsets do not overlap incorrectly and final offset equals total bytes processed.

### D) Async path walker parity test (smoke)

- Replicate one existing sync path walker test case in async mode:
  - symlink depth limiting or nofollow behavior.

---

## Implementation checklist (junior-friendly, in recommended order)

1. **Create `traits_sync.rs` and `traits_async.rs`** and move/re-export the existing traits.
2. **Add missing async traits**: `FsProviderAsync`, `FsAsync`.
3. **Implement adapter families**:
   - sync → async: `Async*FromSync` wrappers using `spawn_blocking`
   - async → sync: `Sync*FromAsync` wrappers using `block_on`
4. **Make mount entries store both interfaces** (or add a wrapper that provides both).
5. **Implement `PathWalkerAsync`** with parity to the current sync walker.
6. **Implement `VfsHandleAsync`** with async mutex and OFD semantics.
7. **Add `VfsDirHandleAsync` and `VfsBaseDirAsync`** for async “at”-style operations.
8. **Add one end-to-end adapter boundary test** for each direction (sync→async and async→sync).

---

## “Update the plan” note (required by `fs-refactor.md`)

When Phase 4 is implemented, update `fs-refactor.md` to record:

- whether the repository kept the existing `node.rs` layout or moved traits into `traits_*.rs`
- which runtime(s) were implemented in `vfs-rt`
- whether async path walking and async OFD semantics shipped in Phase 4 (this spec requires both)
- any limitations (e.g., sync-from-async adapter is documented as potentially deadlock-prone depending
  on caller context)

