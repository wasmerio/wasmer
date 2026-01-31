# Wasmer/Wasix Virtual FS Refactor – Comprehensive Implementation Plan

> Goal: a Linux-compatible, POSIX-semantics VFS layer (mounts, overlays, correct path resolution, correct FD/OFD behavior) implemented in userspace and integrated into Wasix. The core VFS should be "Linux-like" even when specific backends cannot provide every POSIX guarantee (notably object stores). Must support heterogeneous providers (host FS on Linux/macOS/Windows, in-memory FS, overlays, and optional object-store backends). Must provide both sync and async usage paths. Must be modular and extensible. **Performance is critical**: choose low-overhead abstractions, data structures, and caching boundaries from the start.

This plan is written as a step‑by‑step implementation roadmap aimed at junior and senior developers. Each step includes concrete deliverables and integration notes.

## Important constraints (read first)

- **Clean-slate implementation**: we are implementing the new VFS from scratch. We will *integrate* it into `lib/wasix`, but we are not writing a detailed migration/compatibility plan for the old `lib/virtual-fs` stack yet.
- **New crates live under `./vfs/`**: the new implementation is in a new top-level folder to avoid naming/path conflicts. We will remove `lib/virtual-fs` later once the new stack is wired up and stable.
- **Backend reality**: "POSIX semantics" is the target for the VFS layer and for POSIX-capable backends (memfs/hostfs). For non-POSIX backends (object stores), the VFS must be capability-aware and fail fast (or provide explicitly best-effort behavior) rather than silently lying.

---

## General Instructions

- Build discipline: after each step, crates must compile.
- Default command: `cargo check --quiet --message-format=short`.
- If problems are unclear: rerun with `cargo check --quiet` for fuller diagnostics.

- Each phase must have a small, testable acceptance criteria section so work can be handed off cleanly.
- prefer to only check the crates you have modified , or the vfs crates that are affected
- make sure crate sources are well-structured and split into small modules for readability
  no huge files!
- when a step is done, update the plan!
  if you deviated from the plan for some reason or left gaps,
  document it!

---

## Phase 1 – Architecture and Crate Boundaries

### 1.1 Create new crate structure
=> DONE

- **Deliverables**:
  - `vfs/core/` (new; core types, mount table, path walker, common traits)
  - `vfs/mem/` (new; in-memory POSIX-ish filesystem backend, used as reference backend)
  - `vfs/host/` (new; host filesystem backend with internal confinement/sandboxing)
  - `vfs/overlay/` (new; overlay/union implementation or adapter, depending on final layering)
  - `vfs/unix/` (new; syscall-interop helper layer: openat-style helpers, flag translation, errno mapping)
  - `vfs/rt/` (optional but recommended; runtime glue for async<->sync bridging without hard-coding tokio in `vfs/core`)
- Add these crates to the workspace `Cargo.toml` members list (`"vfs/*"` or explicit paths).
- Standardize crate package names to avoid ambiguity in commands and logs:
  - `vfs/core` package name: `vfs-core`
  - `vfs/mem` package name: `vfs-mem`
  - `vfs/host` package name: `vfs-host`
  - `vfs/overlay` package name: `vfs-overlay`
  - `vfs/unix` package name: `vfs-unix`
  - `vfs/rt` package name: `vfs-rt` (if used)
- **Motivation**: Split concerns: core VFS semantics vs backend implementations vs Wasix/WASI translation. Keep runtime coupling (tokio, spawn_blocking, block_on) out of the semantic core.
- **Why `vfs/unix` exists**: to keep `vfs/core` platform-agnostic and to centralize OS/WASI interop (flag translation + a single `wasi::Errno` mapping) so it is not duplicated across `vfs/host` and `lib/wasix`.

### Phase 1 acceptance criteria
- `cargo check -p <modified-crate>` still succeeds after adding empty `vfs/*` crates and wiring them into the workspace.
- `vfs/core` does not depend on tokio (runtime hooks live in `vfs/rt` if needed).
- The provider naming and layering is written down in `vfs/core/README.md` (even if the code is still stubbed).

### 1.2 Define core VFS responsibilities
- **Deliverable**: `vfs/core/src/lib.rs`
- **Core responsibilities**:
  1. Path resolution (including mount traversal and normalization rules).
  2. Mount table semantics (nested mounts, `..` mount-boundary behavior, mount namespaces if needed).
  3. Generic operations required by Wasix' filesystem syscalls: open/openat-like ops, (l)stat, read_dir/getdents, link/unlink, rename, symlink/readlink, mkdir/rmdir, chmod/chown/utimens (as supported).
  4. Core types: `MountId`, `VfsInodeId`, `VfsHandleId`, `VfsFileType`, `VfsMetadata`, `VfsDirEntry`, `VfsError`, `VfsResult`.
  5. Capability model and policy hooks (e.g. allow/deny operations, jail/chroot-ish behavior).

### 1.3 Define filesystem provider abstraction
- **Deliverable**: `vfs/core/src/provider.rs`
- Use Linux‑style terminology:
  - **FsProvider** ≈ filesystem type/driver (`file_system_type` in Linux).
  - **Fs** ≈ filesystem instance/superblock created by a provider.
  - **Mount** ≈ a mountpoint binding an Fs into the VFS namespace.
- Add a provider registry:
  - `FsProviderRegistry` stores providers by name/type.
  - `register_provider(name, provider)` registers at runtime.
  - `mount_with_provider(name, config, target_path, flags)` creates an Fs and attaches it.
- Require providers to declare capabilities:
  - `FsProviderCapabilities` flags (e.g., `WATCH`, `HARDLINK`, `SYMLINK`, `RENAME_ATOMIC`, `SPARSE`, `XATTR`, `FILE_LOCKS`, `ATOMIC_O_TMPFILE`, `CASE_SENSITIVE`, `CASE_PRESERVING`).
  - VFS uses capabilities to gate operations and expose best‑effort semantics to callers.
  - If `WATCH` is absent, VFS provides a polling‑based watcher emulation for that Fs instance.
- Providers must expose both sync and async call paths. Avoid duplicating logic by declaring a single canonical trait and then providing adapters, or by clearly documenting which trait is canonical and how to implement only one of them.
- Provide common type adapters:
  - `struct AsyncAdapter<T: FsProviderSync>` → implements async by blocking off-thread (runtime-provided, see `vfs/rt`).
  - `struct SyncAdapter<T: FsProviderAsync>` → exposes sync APIs via a caller-provided `block_on` hook (runtime-provided, see `vfs/rt`).
- **Design note (important for implementers)**: the async trait surface must be object-safe to support a dynamic/extensible provider registry. We will use `async-trait` for async traits, which lowers methods to boxed futures suitable for `dyn` dispatch. Document this in `vfs/core/README.md` so every crate implements it consistently.
- Providers supply Fs instances that implement primitive ops; POSIX-ish behavior lives in `vfs/core` (semantic rules) plus small helpers in `vfs/unix`.

### 1.4 Define POSIX translation crate
- **Deliverable**: `vfs/unix/src/lib.rs`
- This crate provides small, reusable helpers to avoid duplicating POSIX-ish rules in multiple call sites (especially between `vfs/host` and `lib/wasix`):
  - open/openat-style flag translation (POSIX <-> internal flags)
  - error model + `errno`/`wasi::Errno` mapping helpers (single source of truth for mapping)
  - directory entry encoding helpers (getdents-style)
- Path resolution should live in `vfs/core` so there is a single source of truth for mount/symlink traversal.
- This crate is also the right place to isolate OS-specific and potentially `unsafe` host-interop code (e.g. `openat2`/`openat` fallback helpers), keeping `vfs/core` free of OS-specific dependencies.
- Minimum deliverables inside this crate:
  - `vfs/unix/src/errno.rs`: mapping from `VfsError` to `wasi::Errno` (and optionally to host `std::io::ErrorKind` where needed)
  - `vfs/unix/src/open_flags.rs`: open/openat flag conversion (WASI <-> VFS internal)
  - Document the `VfsError` surface (in `vfs/core`), including:
    - a stable, semantically-meaningful error kind enum (e.g. `NotFound`, `NotDir`, `IsDir`, `AlreadyExists`, `PermissionDenied`, `NotSupported`, `CrossDevice`, `TooManySymlinks`, `InvalidInput`, `WouldBlock`, etc.)
    - an optional underlying source error (host errno / io error) for debugging only

### 1.5 Wasix integration boundary (no new resource crate for now)
- **Deliverable**: `lib/wasix/src/fs/` updates (new module(s), see Phase 5)
- Keep the Wasix resource table in `lib/wasix` for now. Integrate the new VFS by:
  - Using VFS handles for file/dir resources
  - Keeping sockets/pipes/TTY resources as-is
  - Preserving the Wasix rights model, but delegating filesystem semantics to the new VFS
  - Avoiding large-scale re-architecture of unrelated Wasix subsystems in the initial pass

### Layering contract (source of truth rules)

This section is non-negotiable. It exists to prevent filesystem semantics from being re-implemented inconsistently across Wasix, VFS core, and backends.

- **Path resolution**:
  - All traversal semantics live in `vfs/core::PathWalker` (mount transitions, `..`, symlinks, trailing slashes, depth limits).
  - Wasix must not implement its own traversal logic or "relative path hacks" once the new VFS path walker is in use; Wasix only selects flags (e.g. follow symlinks or not) and passes inputs into VFS.
- **Confinement / sandboxing**:
  - Host filesystem confinement must be enforced by `vfs/host` using internal capability-like handles (dirfd/openat-style operations on Unix; handle-based containment on Windows).
  - Wasix must not rely on string prefix checks ("path starts with root") for confinement; this is not safe in the presence of symlinks and races.
- **WASI rights vs POSIX permissions**:
  - Wasix performs WASI/WASIX rights gating first (cheap, deterministic, independent of filesystem state).
  - VFS then enforces POSIX permission checks and mount policies (uid/gid/mode bits, chroot/jail policy hooks) where configured and meaningful.
  - Missing backend features must not be silently "faked"; they must be reflected via capability flags and correct errors.
- **Backend capability truthfulness**:
  - `FsProviderCapabilities` is the contract. If a backend cannot guarantee atomic rename, stable inode identity, hardlinks, etc., it must clear that capability and operations must return correct errors (`EXDEV`, `ENOTSUP`, etc.) or use explicitly-scoped emulation documented per operation.
- **FD vs OFD correctness**:
  - Offset/appending/status flags live in the VFS-level OFD (`vfs/core::VfsHandle`) so `dup` shares offsets correctly.
  - Per-FD flags (`CLOEXEC`) live in Wasix' resource table and must not affect OFD shared state.
- **"at"-style behavior is not optional**:
  - Wasix "at" syscalls (openat, fstatat, unlinkat, etc.) must be implemented as base-directory-handle + relative-path operations; do not convert them into global absolute-path strings.
  - This is required for correctness and for confinement.

---

## Phase 2 – VFS Core Semantics (Clean‑Slate Implementation)

### 2.1 Canonical path normalization and resolution
- **Deliverables**: `vfs/core/src/path_walker.rs`
- Implement path normalization with POSIX semantics:
  - `.` and `..` resolution.
  - Preserve trailing slashes where needed (`stat` vs `open` behavior).
  - Respect `AT_FDCWD` semantics for relative operations.
- Create a dedicated `PathWalker` struct for step‑by‑step traversal.
- Resolution must be mount‑aware (see Phase 3).
- Enforce the layering contract: once `PathWalker` exists, all Wasix filesystem syscalls must route through it (no parallel resolution logic in `lib/wasix/src/fs`).
- Explicitly specify (and test) symlink resolution rules:
  - `O_NOFOLLOW` / `AT_SYMLINK_NOFOLLOW` behavior
  - max symlink depth (Wasix currently uses `MAX_SYMLINKS`; keep a single constant here and reuse it)
  - behavior when encountering a symlink in the last path component vs intermediate components
  - correct error behavior: `ELOOP` when exceeding depth; `ENOTDIR`/`ENOENT`/`EINVAL` for trailing slash mismatches per POSIX rules (document expected outcomes in tests)

### 2.2 Define inode and node identifiers
- **Deliverable**: `vfs/core/src/inode.rs`
- Use small, cache-friendly IDs:
  - `MountId`: `u32` (or `NonZeroU32`)
  - `BackendInodeId`: `u64` (backend-defined, stable for the lifetime of the mount)
  - `VfsInodeId`: `(MountId, BackendInodeId)`
- Add per‑mount inode namespace to avoid collisions across mounts.
- For object stores or backends without inode semantics:
  - Use a backend-managed mapping table from backend keys (bucket/key/version) to `BackendInodeId` to avoid hash collisions and to support explicit invalidation.
  - Document the "stability contract": inode identity must be stable within a single mount lifetime; persistence across process restarts is not required.

### 2.3 Define filesystem node interfaces
- **Deliverable**: `vfs/core/src/node.rs`
- An `FsNode` (backend node handle) represents a file/dir/symlink inside a specific mounted filesystem instance.
- Provide operations (sync + async):
  - `lookup`, `read_dir`, `create`, `remove`, `rename`, `link`, `symlink`, `readlink`, `set_metadata`, `get_metadata`, `open`.
- Nodes are obtained from an `Fs` instance by:
  - root node lookup
  - directory lookup (parent node + name)
  - handle-based operations (e.g. `openat` uses a directory handle as a base)
- **Design constraint (important)**: backends must not rely on global absolute paths for correctness. For hostfs this means using internal dir-handle/openat-style operations to enforce confinement and avoid symlink escapes.

### 2.4 File handles and open file descriptions
- **Deliverable**: `vfs/core/src/handle.rs`
- Implement Open File Description (OFD) semantics:
  - An open handle holds position/offset, flags, and file description state.
  - Multiple file descriptors may share an OFD (`dup`, `fork` semantics).
- Provide typed `VfsHandle` with sync + async IO methods. `VfsHandle` wraps an underlying backend handle (e.g. `FsHandle*`) and adds OFD state and VFS-level policy enforcement.
- Make the split explicit in the API:
  - OFD-like object: shared state (offset, append mode, status flags)
  - FD-like object: per-descriptor flags (`CLOEXEC`) live in Wasix' resource table (Phase 5)

### 2.5 Metadata and permission model
- **Deliverable**: `vfs/core/src/metadata.rs`
- Implement full POSIX `stat` support: mode, inode, device, link count, uid/gid, size, atime/mtime/ctime.
- Provide per‑mount policy hooks for permission checks and jail/chroot semantics.
- Clarify division of responsibility:
  - VFS enforces POSIX permission model (where configured and meaningful).
  - Wasix enforces WASI/WASIX rights (independent, additional gate).
- Define explicit evaluation order for operations that can fail both ways:
  - Wasix rights checks happen first.
  - Then VFS policy/perms checks.
  - Then backend operation.
  - Error mapping to `wasi::Errno` happens once, in `vfs/unix` helpers (no ad-hoc mapping scattered across call sites).

### Phase 2 acceptance criteria
- `vfs/core` + `vfs/mem` compile and contain enough functionality to:
  - create a root filesystem in tests
  - create files/dirs, write/read bytes, and return `stat` metadata
  - resolve paths correctly for `.` and `..` (including trailing slash behavior)
- A test module exists that exercises symlink depth limiting and `NOFOLLOW` behavior (even if the hostfs is not implemented yet).
- A "green bar" command is documented and works locally:
  - `cargo test -p vfs-core`
  - `cargo test -p vfs-mem`

---

## Phase 3 – Mount Table and Overlay Support

### 3.1 Mount table design
- **Deliverable**: `vfs/core/src/mount.rs`
- Implement hierarchical mount table:
  - Supports nested mounts, overlay mounts, bind mounts, and mount options.
- Each mount is a `MountPoint` with:
  - target path, Fs instance, root inode, mount flags, and namespace id.
- Runtime mount flow:
  - `register_provider(name, FsProvider)` in a global or per‑context registry.
  - `mount(provider_name, provider_config, target_path, flags)` creates Fs instance and attaches it.
- Runtime unmount flow:
  - `unmount(target_path, flags)` detaches the mountpoint.
  - Unmount must handle busy mounts (open handles/watchers) with Linux‑like semantics:
    - Default: fail with `EBUSY`.
    - Optional: lazy unmount (`MNT_DETACH` behavior) where the mount is detached from the namespace but stays alive until last handle closes.
- **Implementation note**: avoid O(n) mount lookups on every path component. Pick a data structure that makes "longest prefix match" fast (e.g. a path trie, radix tree, or a sorted vec with binary-search on prefix boundaries). Document the chosen approach and its complexity in `vfs/core/src/mount.rs`.
- **Chosen approach (performance + correctness)**: mount transitions are inode-driven, not string-prefix driven.
  - Store mounts keyed by their mountpoint inode in the parent mount:
    - `mount_by_mountpoint: HashMap<VfsInodeId, MountId>`
    - `mounts: SlotMap/Vec/Slab<MountId, MountEntry>` where `MountEntry` stores `parent_mount`, `mountpoint_inode`, `root_inode`, `fs`, and flags.
  - Hot path in `PathWalker` becomes:
    1) lookup next component in the current directory to get a `VfsInodeId`
    2) do a single O(1) hash lookup in `mount_by_mountpoint` to see if that inode is a mountpoint
    3) if yes, transition to the mounted FS root inode
  - `..` across mount boundaries:
    - when resolving `..` from a mounted FS root, transition to `parent_mount` and the `mountpoint_inode` (Linux-like).
  - Concurrency strategy:
    - reads are hot and must be lock-light: represent the mount table as an immutable snapshot `Arc<MountTableInner>` and use a swap-on-update mechanism (e.g. `ArcSwap`) for mount/unmount.
    - mount/unmount create a new snapshot and atomically publish it; existing path walkers continue on the old snapshot safely.
  - Rationale: avoids tricky path-string normalization interactions (symlinks, multiple textual paths), matches Linux VFS semantics, and is faster than prefix matching.

### 3.2 Provider registry (explicit)
- **Deliverable**: `vfs/core/src/provider_registry.rs`
- The VFS layer owns a registry of available FsProviders.
- Providers are registered at runtime. Prefer a per-context registry owned by the Wasix environment to avoid cross-test interference and unintended global state.
- Mounting uses the registry to locate a provider by name/type and construct an Fs instance.
- Registry also exposes provider capabilities for validation and error messaging before mount.

### 3.3 Path resolution with mounts
- Integrate mount traversal into `PathWalker`:
  - On each path component, check for mount transitions.
  - Maintain `current_mount`, `current_inode`.
- Ensure correct behavior for `..` across mount boundaries.

### 3.4 Overlay/union filesystem
- **Deliverable**: `vfs/overlay/src/lib.rs` (or `vfs/core/src/overlay.rs` if overlay is a core feature)
- Implement overlay mount type:
  - Multiple lower layers (read‑only), single upper layer (read‑write).
  - Copy‑up on write semantics.
  - Whiteouts for deletions.
- Define explicit `OverlayFs` adapter that presents a unified `Fs`.
- Define the initial overlay semantics target explicitly (do not leave this ambiguous); keep the spec in this plan (and optionally extract into `docs/fs_refactor/requirements.md` later) and treat tests as the executable source of truth.
  - Required (Wasix v1 overlay fidelity target):
    - Directory merge rules: upper shadows lower for same name; `readdir` returns a merged view (upper first, then lower entries not shadowed by upper/whiteouts; no implicit sorting).
    - Whiteouts (tombstones): deleting a lower-only entry must hide it in the merged view until the overlay is torn down.
      - Representation is an internal overlay concept (do not require Linux overlayfs char-dev whiteouts); implement via explicit tombstone metadata in the upper layer and/or an overlay-side tombstone map.
      - `unlink` / `rmdir` on a lower-only entry creates a whiteout rather than mutating the lower layer.
      - If the chosen upper backend cannot persist overlay metadata natively (e.g. hostfs), the overlay must store its metadata in an internal sidecar area within the upper (e.g. a hidden directory) or in an overlay-managed metadata file; document the exact scheme and ensure it is confinement-safe.
    - Opaque directories: support “hide everything from lower under this directory” (required for container/OCI-style layering semantics).
      - Representation is internal overlay metadata (do not require Linux overlayfs xattrs); implement via an explicit “opaque marker” in upper/overlay metadata.
    - Copy-up:
      - Copy-up happens before the first operation that would mutate a lower-only node (data writes, truncate, chmod/chown/utimens, link/rename that mutates source metadata).
      - For correctness and to avoid mid-handle copy-up complexity, prefer copy-up at `open` time when flags indicate write intent (`O_WRONLY`, `O_RDWR`, `O_TRUNC`, `O_CREAT`) rather than lazily during later `write`.
    - Rename semantics (must be explicit and tested):
      - `rename` across mounts returns `EXDEV` (handled by the mount table, not overlay).
      - File rename within an overlay mount is supported and may trigger copy-up of a lower-only file prior to rename.
      - Directory rename where the source exists only in lower is initially **not supported** (return `ENOTSUP`); document this limitation and add a targeted test that asserts the chosen errno.
    - Error behavior for unsupported features is consistent and mapped once to WASI (`vfs/unix/src/errno.rs`).
  - Optional / phase-later (explicitly out-of-scope for v1):
    - Metadata-only copy-up (“metacopy”): allowing chmod/chown/utimens on a lower-only file without copying file data.
      - If/when implemented, define the exact semantics (which metadata fields are virtualized) and failure modes.
    - Linux overlayfs parity details: redirect_dir, index, nfs_export, xino, copy-up corner cases around open file handles.
    - xattrs/ACLs merging rules and whiteouts encoded as host-visible special files (unless a concrete persistence/import/export requirement exists).
- Common container‑style layout:
  - Virtual root is a read‑only base (packages, runtime layers).
  - Upper layer is a writable tmpfs or memfs with strict limits.
  - Select paths are bind‑mounted to persistent host directories for state.

### 3.5 Special filesystems
- Provide placeholder internal FS for:
  - `proc`‑like FS for runtime introspection.
  - `dev` FS for device nodes, pipes, sockets (logical representation).
  - `tmpfs`‑style in‑memory FS.

### 3.6 Filesystem usage limits and quotas
NOTE: limits and quotas are optional and not required
there should be a no/zero cost flow if no limits/quotas are configured.

- **Deliverable**: `vfs/core/src/limits.rs`
- Add per‑Fs and per‑mount limit configuration:
  - Max used bytes, max inodes, max directory entries, max path length.
    each should be optional
  - Optional per‑uid/gid quotas for multi‑tenant use.
- Add global accounting to enforce limits consistently across overlay layers.
- For overlay mounts, enforce limits in the upper (writable) layer to prevent unbounded in‑memory growth.
- Expose limit configuration via provider config and mount flags.
- expose a hook to have shared accounting across multiple Fs instances, or even across domains (e.g. network + FS + other resources)

### 3.7 IO rate limiting (IOPS/throughput)
- **Deliverable**: `vfs/ratelimit/`
- Provide a standalone limiter abstraction usable across subsystems:
  - `RateLimiter` trait with sync + async interfaces.
  - Pluggable implementations (token‑bucket, leaky‑bucket, fair‑queue).
- Support IOPS and bandwidth throttling:
  - Per‑mount limits (applies to all Fs instances at that mountpoint).
  - Per‑Fs limits (applies to a specific filesystem instance).
  - Global VFS limits (applies to all operations in a process/context).
- Implement token‑bucket (or leaky‑bucket) schedulers for sync and async paths.
- Provide burst configuration and fairness controls (round‑robin or weighted by priority).
- Support throttling semantics that can **wait for capacity**:
  - Async: `await` until tokens are available (with cancellation support).
  - Sync: block with optional timeout or return `EAGAIN` when non‑blocking.
- Surface throttling errors as `EAGAIN`/`EWOULDBLOCK` when requested by flags; otherwise apply back‑pressure.
- Make the limiter pluggable to integrate with a wider system‑wide rate limiter (network + FS + other resources).
- Integrate limiter hooks in VFS op dispatch so every IO path can be governed with minimal overhead.

### Phase 3 acceptance criteria
- Nested mounts work for the memfs backend and path resolution crosses mount boundaries correctly:
  - entering a mount at its mountpoint works
  - `..` at a mount root behaves correctly
- Cross-mount rename fails with `EXDEV`.
- If overlay is implemented in this phase: basic copy-up + whiteout behavior is covered by tests.

---

## Phase 4 – Sync + Async Interface Design

Async implementation rule: all async traits in `vfs/*` use `async-trait` to remain object-safe and usable via `dyn` dispatch in the provider registry.

### 4.1 Sync trait API
- **Deliverable**: `vfs/core/src/traits_sync.rs`
- `FsProviderSync` for mounting.
- `FsSync` for `stat`, `open`, `read_dir`, etc.
- `FsNodeSync` for node operations.
- `FsHandleSync` for `read_at`, `write_at`, `seek`, `flush`, `fsync`.

### 4.2 Async trait API
- **Deliverable**: `vfs/core/src/traits_async.rs`
- `FsProviderAsync` with async mount equivalents (if needed).
- `FsAsync` with `async fn` equivalents.
- `FsNodeAsync` / `FsHandleAsync`.
- Ensure interfaces are `Send + Sync` where appropriate.

### 4.3 Adapters and ergonomics
- Provide adapters and default implementations for backends that only implement one of the two trait sets.
- Provide explicit runtime hooks for async<->sync bridging in `vfs/rt`:
  - spawn-blocking implementation (for sync backends exposed as async)
  - block-on implementation (for async backends exposed as sync)

### Phase 4 acceptance criteria
- One backend can be implemented using only the sync traits and still be usable from the async interface (via adapters), and vice-versa.
- The chosen async-trait strategy is implemented and covered by at least one unit test exercising a sync<->async adapter boundary.

### Phase 4 implementation status (2026-01-31)
- Traits are centralized in `vfs/core/src/traits_sync.rs` and `vfs/core/src/traits_async.rs`;
  `node.rs` re-exports for compatibility.
- `vfs-rt` now provides `InlineTestRuntime` and a `TokioRuntime` behind the `tokio` feature.
- Mount table entries store both sync and async `Fs` interfaces; adapters are used at mount time.
- Async path walker and async OFD handle (`VfsHandleAsync`) are implemented using an async mutex.
- Sync-from-async adapters are supported but remain caller-controlled (may deadlock if used inside
  a single-threaded async runtime without spawn-blocking).

---

## Phase 5 – Wasix Integration (syscalls + resource table)

### 5.1 Define the new Wasix<->VFS interface surface
- **Deliverable**: `lib/wasix/src/fs/` new module(s) (e.g. `lib/wasix/src/fs/vfs.rs`)
- Define a narrow interface that `lib/wasix/src/syscalls/*` uses for filesystem operations:
  - "at"-style operations (openat, fstatat, unlinkat, mkdirat, renameat, symlinkat, readlinkat, etc.)
  - conversions between WASI rights/flags and VFS flags
  - `Errno` mapping (VFS error -> `wasi::Errno`)
- This layer should not embed filesystem semantics; it should call into `vfs/core` (and `vfs/unix` helpers).
- Explicitly remove/avoid path-resolution duplication:
  - do not keep `relative_path_hack`-style logic for VFS-backed operations
  - do not resolve symlinks in Wasix; pass flags to `PathWalker` instead
- "at"-style base handling requirements:
  - base FD must map to a directory handle / node reference in VFS; do not convert base+relative into an absolute path string in Wasix

### 5.2 Attach VFS handles to resources
- **Deliverable**: `lib/wasix/src/fs/` (update existing FD/inode/resource types)
- Define a Wasix file/dir resource that holds:
  - a VFS handle (OFD-like state) for open files
  - a VFS node reference / inode id for directories and path-based operations
- Ensure FD behavior matches POSIX requirements where applicable: `O_CLOEXEC`, `O_APPEND`, `O_NONBLOCK`, etc.

### 5.3 Ensure FD semantics match POSIX
- Support `dup`, `dup2`, `close`, `fcntl`, and per‑descriptor flags.
- Share OFD state where POSIX requires it.
- Acceptance criteria: add focused tests in `lib/wasix` for the FD/OFD invariants that previously broke (dup shares offset; close-on-exec only per-FD; etc.).

### 5.4 Wasix API bridging
- Update `lib/wasix/src/fs` to become a thin shim over VFS core and resource table.
- Keep syscall mapping logic in Wasix but remove filesystem business logic.
- (Optional) Integrate resource-level rate limiting hooks if `vfs/ratelimit` exists.

### Phase 5 acceptance criteria
- A minimal set of Wasix filesystem syscalls are served by the new VFS (behind a feature flag or a build-time switch if necessary):
  - open/read/write/close
  - (l)stat
  - readdir/getdents
  - mkdir/rmdir/unlink
- The old code remains buildable until the new implementation is feature-complete enough to remove it.
- A "green bar" command is documented and works locally:
  - `cargo test -p wasmer-wasix` (or a narrower package/test selection if full suite is too slow)

---

## Phase 6 – Provider Integration Layer

### 6.1 Host filesystem provider
- **Deliverable**: `vfs/host/src/lib.rs`
- Provide adapters for Linux/macOS/Windows.
- Implement internal confinement/sandboxing in `vfs/host` (do not use `cap-std`).
- Support `openat`-style semantics on Unix (prefer `openat2` where available; otherwise use best-effort `openat` + additional checks) and equivalent handle-based containment on Windows.
- Support `O_PATH` where possible, and provide clear fallbacks where unavailable.
- Provide fallbacks where the host OS lacks semantics.
- Include native file watch integration where possible (inotify/FSEvents/ReadDirectoryChangesW), surfaced through the VFS watch API.
- Confinement requirements (security-critical):
  - enforce confinement using capability handles (dirfd/openat-style), not string prefix checks
  - ensure symlink traversal cannot escape the sandbox root via races or `..` tricks
  - document platform differences clearly (Windows path/case semantics, macOS quirks)

### 6.2 In‑memory filesystem provider
- **Deliverable**: `vfs/mem/src/lib.rs`
- Fully in‑memory, POSIX‑compliant semantics (tmpfs-like).
- Primary roles:
  - reference backend for correctness tests (fast iteration, deterministic behavior)
  - a default “virtual rootfs” for fully in-memory environments
  - an overlay upper layer when you want copy-up semantics without touching the host FS
- Data structures:
  - Prefer deterministic directory enumeration for tests and reproducibility.
  - Initial implementation may use `BTreeMap` for inode tables / dir entries for determinism; if performance requires it later, switch to `hashbrown` + an explicit stable ordering strategy for `readdir`.
- Support hard links and symlinks.
- Integrate limits:
  - enforce max-bytes / max-inodes from `vfs/core/src/limits.rs` (or an equivalent local module) so Wasix can cap memory growth.
- Hybrid / spill-to-disk (phase-later; do not block v1):
  - If we need “keep metadata in memory but offload file bodies to disk under pressure”, implement this as either:
    - a separate provider (e.g. `vfs/tiered` / `vfs/cache`) that composes `memfs` + a confined `hostfs` directory as backing store, or
    - a pluggable `FileDataStore` inside `memfs` that can move cold file contents to a backing store while keeping inode/dir metadata in memory.
  - Define explicit semantics before implementing spill (open handles pin file data; eviction is best-effort; offloading must not break POSIX-visible behavior).

### 6.3 Network filesystem providers
- Defer to a later milestone. The provider APIs must not preclude this, but the initial pass should not be blocked on SMB/NFS.

DEFERRED
<!-- ### 6.4 Object store provider (S3, etc.) -->
<!-- - **Deliverable**: `vfs/object-store/` (optional; only build when needed) -->
<!-- - Use an object store crate (e.g. `object_store` or `opendal`) under a directory abstraction. -->
<!-- - Implement pseudo‑directories by prefix matching. -->
<!-- - Enforce immutable/append semantics and eventual consistency. Expose limitations via capabilities and return `ENOTSUP` where appropriate. -->
<!-- - Provide a synthetic inode mapping cache for objects. -->
<!-- - Provide a polling‑based watcher with configurable interval and incremental listing for change detection. -->

### 6.5 Overlay + union provider
- **Deliverable**: `vfs/overlay/src/lib.rs`
- Implement overlay logic as an `Fs` adapter (preferred) or a provider, depending on how mounts are represented in `vfs/core`.

---

## Phase 7 – POSIX Semantics Compliance

Test placement and execution rules:
- Core VFS correctness tests live in `vfs/core/tests/` (and unit tests in-module as appropriate).
- Backend-specific tests live in the backend crate (e.g. `vfs/mem/tests/`, `vfs/host/tests/`).
- Wasix integration tests live in `lib/wasix` (prefer `lib/wasix/tests/` where feasible) and must not duplicate path resolution logic.
- Default commands:
  - `cargo test -p vfs-core`
  - `cargo test -p vfs-mem`
  - `cargo test -p wasmer-wasix`

### 7.1 Path resolution correctness tests
- Implement an extensive path resolution suite:
  - `..` crossing mount boundaries
  - symlink loops and depth limits
  - `openat` relative resolution
  - trailing slash behavior

### 7.2 File descriptor semantics
- Implement tests for:
  - shared offsets (dup)
  - `O_APPEND`
  - `O_TRUNC`
  - `O_NONBLOCK`
  - `O_CLOEXEC`

### 7.3 Metadata and permission tests
- Ensure mode bits, uid/gid, and time fields behave correctly.
- Validate `chmod`, `chown`, `utimensat` behavior.

### 7.4 Rename/link semantics
- Strictly enforce POSIX rename atomicity where possible.
- Ensure correct behavior when moving across mounts.
  - Cross-mount rename must return `EXDEV`.

### 7.5 File system watching semantics
- Define watch behavior for files and directories: create, delete, modify, rename, attribute changes.
- Ensure watch events are ordered and de‑duplicated where required by API.
- Provide test fixtures for recursive watches and mount overlays.

---

## Phase 8 – Performance and Reliability

### 8.1 Caching layer
- Provide optional metadata cache to improve performance for network and object stores.
- Use LRU or TTL caches with explicit invalidation rules.

### 8.2 Concurrency and locking
- Define locking strategy for global mount table, inode table, and fd table.
- Provide `RwLock` on mount table with minimal lock contention.

### 8.3 Stress and fault testing
- Implement fault injection tests for network and object stores.
- Test concurrency, race conditions, and failure modes.

### 8.4 Performance‑first data structures and APIs
- Use cache‑friendly maps for hot paths (e.g., `hashbrown`/`DashMap` for inode and entry tables).
- Avoid excessive allocation in path resolution (pre‑allocate component buffers, reuse scratch space).
- Keep handle and inode objects small and copyable where possible (IDs are opaque, metadata fetched lazily).
- Provide batched directory reads and async read‑ahead hooks for high‑latency backends.
- Instrument critical paths with lightweight tracing and counters for profiling.
- Add fast‑path limit checks for in‑memory FS allocations to avoid lock contention.
- Ensure rate‑limiter checks are low‑overhead and avoid global contention on high IOPS workloads.

---

## Phase 9 – Documentation and Adoption

### 9.1 Developer guide
- Write developer guide and architecture docs in `docs/fs_refactor/`.
- Provide API docs for backends and VFS core.

---

## File System Watching – API and Semantics

NOTE: This subsystem is optional in the initial implementation pass unless a concrete Wasix feature requires it immediately.

### Watcher abstraction (core)
- **Deliverable**: `vfs/core/src/watch.rs`
- Introduce `VfsWatchEvent` and `VfsWatchKind` (create/modify/delete/rename/attrib/overflow).
- Provide `VfsWatcherSync` and `VfsWatcherAsync` traits that are **wired into Fs/FsProvider** so native backends can surface their own watcher handles.
- Each `Fs` instance exposes `watcher_sync()` / `watcher_async()` (or returns `Option<...>` if unsupported).
- Watchers are registered against a `VfsInodeId` (preferred) or a path plus flags (recursive, follow symlinks, etc.).

### Watch integration with mounts and overlays
- Events must be re‑mapped from backend paths/inodes to VFS paths and mount namespaces.
- Overlay mounts must emit events for changes in upper layer; lower layer changes are best‑effort depending on backend capabilities.
- When watching a merged view, events include layer info (upper/lower) in metadata.

### Event queueing and delivery
- Use per‑watch queues with bounded capacity and overflow events.
- Provide optional coalescing of repeated events.
- Ensure watchers are cancelable and closed on resource drop.

### Providers without native watching
- Provide fallback poller with configurable intervals and hashing/mtime heuristics.
- For object stores, use object listing + ETag/version checks.
- For network FS, respect server‑side watch APIs when available; otherwise, poll.

### Resource table integration
- Expose watcher handles as resources in `lib/wasix` (same resource table as file/dir handles).
- Provide WASIX‑level APIs for registering and consuming watch events, with async and sync interfaces (or reuse an existing notification mechanism if one exists).

## Detailed Implementation Order (Step‑by‑Step)

1. **Create crate skeletons** for `vfs/core`, `vfs/mem`, `vfs/host`, `vfs/unix` (and optionally `vfs/rt`).
2. **Add core type definitions** (`VfsInodeId`, `VfsMetadata`, `VfsHandle`, `VfsError`) in `vfs/core`.
3. **Lock in async-trait usage** for all async traits (object-safe dyn dispatch) and add adapters/runtime glue (`vfs/rt` if used).
4. **Implement path normalization and PathWalker** (symlink rules + mount awareness).
5. **Implement mount table** and provider registration/mount API.
6. **Implement memfs** (the reference backend) and use it to build a correctness test suite.
7. **Implement file handle semantics** (OFD state; offset sharing; append).
8. **Implement directory traversal and getdents-style encoding**.
9. **Integrate into Wasix**: add a new VFS-backed FS module in `lib/wasix/src/fs` and route syscalls through it.
10. **Implement hostfs** with internal confinement/sandboxing and platform-specific fallbacks.
11. **Implement overlay** if required by Wasix rootfs layering use-cases.
12. **Optional**: object-store backend, watch subsystem, quotas, rate limiting.
13. **Run and expand tests**: correctness first, then performance.

---

## Implementation Details – Key Modules

### A. Path Resolution and Mounts
- `PathWalker::resolve(base_fd, path, flags)`
  - Accepts base file descriptor (or AT_FDCWD), path string, and resolution flags.
  - Walks components step‑wise.
  - Detects mount transitions at each component.
  - Supports symlink resolution with a fixed maximum depth (reuse the constant defined for Wasix, e.g. `MAX_SYMLINKS`).
  - Preserves trailing slash requirements.

### B. Inode Handling
- Each mount has `MountId` and inode namespace.
- `VfsInodeId` is tuple `(MountId, BackendInodeId)`.
- Backends that lack inode IDs provide synthetic ones.

### C. Overlay Semantics
- Goal: a pragmatic, fast overlay sufficient for Wasix rootfs layering and typical application I/O, without chasing full Linux overlayfs implementation parity.
- Read path:
  - Lookup order: upper first; if not present and not whiteouted, fall through to the first lower that contains the entry.
  - Directories present in multiple layers are merged unless the upper marks the directory opaque.
  - `readdir` merges entries as: upper entries first, then lower entries not shadowed by upper names or whiteouts; no sorting is required, but results must be deterministic for a fixed backend state.
- Whiteouts (tombstones):
  - Deleting a lower-only entry creates a tombstone so the merged view reports `ENOENT`.
  - Tombstones are an internal overlay feature (do not require Linux overlayfs device nodes); they may be stored as:
    - explicit metadata in the upper backend (memfs can store tombstones directly), and/or
    - an overlay-maintained tombstone map (non-persistent) when the upper backend cannot store custom metadata.
- Opaque directories:
  - An opaque marker on an upper directory blocks all lower entries for that directory (required for container/OCI-style layer semantics).
  - Opaque markers are internal overlay metadata (not xattrs by default).
- Copy-up:
  - Copy-up before first mutation of a lower-only node. Prefer copy-up at `open` time for write-intent flags to keep handle semantics simple and avoid mid-stream copy-up.
  - v1 copies both data and metadata. Metadata-only copy-up (metacopy) is phase-later.
- Rename semantics:
  - Cross-mount rename is handled by the mount layer and returns `EXDEV`.
  - Overlay-internal rename of lower-only files triggers copy-up then rename.
  - Overlay-internal rename of lower-only directories is v1 `ENOTSUP` (explicitly tested).
- Inodes:
  - Provide stable inode mapping within a layer; do not promise Linux overlayfs inode behaviors across layers unless explicitly required by Wasix.

### D. Mixed Sync/Async
- All VFS types have `*_sync` and `*_async` variants.
- Callers choose based on runtime context.
- Default to async for network/object store backends.

---

## Expected Outputs by End of Refactor

- A clean and modular VFS core crate with Linux‑like semantics.
- A backend layer supporting host FS, memory FS, overlay FS, and object store FS.
- Fully POSIX‑compliant path resolution and FD behavior.
- Wasix resource system refactored to be more consistent with POSIX semantics.
- Clear and comprehensive documentation and tests.

---

## Next Actions for the Team

- Assign owners per phase.
- Prioritize Phase 1 and 2 for immediate execution.
- Begin adding tests as each subsystem is introduced.
