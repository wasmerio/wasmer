# Spec 6.2 — In-memory filesystem provider (`vfs-mem`)

## Status

- **Target plan section**: `fs-refactor.md` → **Phase 6.2 In‑memory filesystem provider**
- **This spec assumes**:
  - **Phase 4 is complete** (sync/async traits + adapters are in-tree).
  - **Phase 5 (Wasix integration)** is tracked separately; `vfs-mem` must remain usable without `lib/wasix`.
  - We **must not change the existing `vfs-core` public API surface** (traits, structs, type names).

## Summary (one paragraph)

Turn `vfs-mem` into the **reference POSIX-ish backend**: a fast, deterministic, fully in-memory filesystem that is mountable through the provider registry (`FsProviderSync`) and that implements core semantics required by the VFS layer (stable inode identity, correct `unlink` vs open-handle lifetime, hardlinks, symlinks, rename semantics, and read-only mounts). `vfs-mem` remains **sync-only** and relies on Phase 4 adapters for async usage.

## Current implementation baseline (what exists today)

`vfs/mem/src/lib.rs` currently provides:

- `MemFs: FsSync` (via `crate::Fs` re-export)
- `MemNode: FsNodeSync` (via `crate::node::FsNode`)
- `MemHandle: FsHandleSync` (via `crate::node::FsHandle`)

Properties and gaps (relative to the Phase 6.2 target):

- **No provider wrapper**: there is no `MemFsProvider: FsProviderSync`, so it can’t be registered/mounted via `FsProviderRegistry`.
- **No mount flags**: `MountFlags::READ_ONLY` is not enforced (mutating ops do not return `VfsErrorKind::ReadOnlyFs`).
- **No hardlinks**: `FsNode::link()` returns `NotSupported`; `nlink` is always `1`.
- **Open-unlinked lifetime**: the “inode survives while open” behavior is not explicitly designed/tested.
- **Metadata**: fields exist and are editable, but timestamps/uid/gid are currently simplistic defaults and there is no clear “clock” model.
- **Deterministic readdir**: it uses `BTreeMap` so results are deterministic (good for tests).

## Non-goals (explicitly out of scope for 6.2)

- **No persistent storage** (no on-disk backing, no snapshotting/serialization, no journaling).
- **No watch API**: do not claim `FsProviderCapabilities::WATCH`.
- **No full Linux tmpfs feature set**: xattrs/ACLs, `O_TMPFILE`, file locks, and special device nodes are out-of-scope unless `vfs-core` grows the required trait surface.
- **No async-native implementation**: async usage is via Phase 4 adapters (`AsyncFsFromSync`, etc.).

## Goals and acceptance criteria

### Core goals

- Provide a registry-visible provider:
  - `MemFsProvider: FsProviderSync` with provider name **exactly** `"mem"`.
- Provide an explicit config type used for mounting (even if initially empty):
  - `MemFsConfig: ProviderConfig` (via the blanket impl).
- Enforce mount flags:
  - `MountFlags::READ_ONLY` must cause all mutating operations to return `VfsErrorKind::ReadOnlyFs`.
- Implement “POSIX-ish” correctness features:
  - **Hardlinks** (at least for regular files) and correct `nlink` reporting.
  - **Unlink semantics**: unlink removes directory entry; the inode’s content remains accessible via already-open handles until last handle is dropped.
  - **Rename semantics**: atomic within the FS instance (under locks), including `RenameOptions::noreplace` behavior; `exchange` remains `NotSupported` unless implemented.
- Preserve test-friendly determinism:
  - `read_dir` ordering must remain deterministic for a stable FS state.

### Compatibility goals (with `vfs-core`)

- Remain compatible with the current `vfs-core` model:
  - `VfsMetadata.inode.mount` cannot be correctly set by backends today; follow the existing convention (`MountId::from_index(0)`) and document this as a known limitation to be tightened once mount-aware metadata plumbing is finalized.

### Acceptance criteria (tests)

Add tests in `vfs/mem/tests/` (or unit tests in-module) that demonstrate:

- Provider registry mount:
  - `FsProviderRegistry::register_provider("mem", MemFsProvider)` works.
  - `create_fs_sync("mem", …)` returns an `FsSync` whose `provider_name()` is `"mem"`.
- File CRUD:
  - `create_file`, `open`, `write_at`, `read_at`, `set_len`, `len`.
- Directory semantics:
  - `mkdir`, `rmdir`, `DirNotEmpty`.
  - `read_dir` cursor correctness (paging via `VfsDirCookie`).
- Rename:
  - `rename` succeeds and is visible via `lookup`.
  - `RenameOptions::noreplace` returns `AlreadyExists`.
  - `RenameOptions::exchange` returns `NotSupported` (until implemented).
- Symlinks:
  - `symlink` + `readlink` roundtrip.
- Hardlinks:
  - `link` increments `nlink`.
  - unlinking one name decrements `nlink` but the inode persists while a handle is open.
- Read-only mount:
  - With `MountFlags::READ_ONLY`, all mutating operations (`create_file`, `mkdir`, `unlink`, `rmdir`, `rename`, `symlink`, `set_metadata.size`, handle `write_at`, `set_len`) return `ReadOnlyFs`.

## Deliverables (files and modules)

### Required files

- `vfs/mem/src/lib.rs` — crate entry; exports provider/config/fs types.

### Recommended internal module layout (to keep files small)

Refactor `vfs/mem/src/lib.rs` into:

- `lib.rs` — exports and module wiring
- `config.rs` — `MemFsConfig` (limits, determinism options)
- `provider.rs` — `MemFsProvider` (`FsProviderSync`)
- `fs.rs` — `MemFs` (`FsSync`) + shared state (`MemFsInner`)
- `inode.rs` — inode table representation and `BackendInodeId` allocation
- `node.rs` — `MemNode` (`FsNodeSync`) with directory operations
- `handle.rs` — `MemHandle` (`FsHandleSync`) and open-unlinked lifetime rules

## Public API of `vfs-mem` (crate exports)

The crate should export a small stable surface:

```rust
pub struct MemFsProvider;

#[derive(Clone, Debug, Default)]
pub struct MemFsConfig {
    /// Optional max bytes allowed for all file data in this FS instance.
    pub max_bytes: Option<u64>,
    /// Optional max inode count allowed in this FS instance.
    pub max_inodes: Option<u64>,
    /// If true, `read_dir` ordering is stable/deterministic (recommended for tests).
    pub deterministic_readdir: bool,
}

#[derive(Debug)]
pub struct MemFs;
```

Notes:

- If `vfs-core` later grows a shared limits API (e.g. `vfs/core/src/limits.rs`), `MemFsConfig` should migrate to use it, but **6.2 must not require adding new `vfs-core` public API**.

## Provider capability plan

### `FsProviderCapabilities` (registry-facing)

`MemFsProvider::capabilities()` should advertise what the implementation truly supports:

- **Must include**:
  - `FsProviderCapabilities::SYMLINK`
  - `FsProviderCapabilities::HARDLINK` (once implemented)
  - `FsProviderCapabilities::UNIX_PERMISSIONS` (metadata fields are supported in-memory)
  - `FsProviderCapabilities::UTIMENS` (once times are supported consistently)
  - `FsProviderCapabilities::STABLE_INODES` (inodes are stable within the mount lifetime)
  - `FsProviderCapabilities::CASE_SENSITIVE`
  - `FsProviderCapabilities::CASE_PRESERVING`
  - `FsProviderCapabilities::SEEK`
- **Must not include**:
  - `WATCH` (until a watch API exists in `vfs-core`)
  - `XATTR`, `FILE_LOCKS`, `O_TMPFILE` unless explicitly implemented and tested

### `VfsCapabilities` (Fs-instance-facing)

`MemFs::capabilities()` should return coarse VFS-level capabilities:

- `VfsCapabilities::SYMLINKS`
- `VfsCapabilities::HARDLINKS` (once implemented)
- `VfsCapabilities::CHMOD`, `VfsCapabilities::CHOWN`, `VfsCapabilities::UTIMENS` (once these are fully supported and tests exist)

## Semantics and data model

### Inode identity and stability

- Inodes are represented by `BackendInodeId` allocated from a monotonic counter (never zero).
- Inode identity must be stable for the lifetime of the FS instance.
- Hardlinks require multiple directory entries to point at the same inode.

### Open file descriptions and unlink behavior

To match POSIX expectations (and to serve as a correctness reference for `vfs-core` handle/OFD semantics):

- `unlink(name)` removes the **directory entry** immediately.
- If there are open handles referencing the inode:
  - the inode’s data must remain accessible via those handles
  - the inode is only freed once both:
    - `nlink == 0` and
    - the last open handle is dropped

Implementation direction:

- Store inode state in `Arc<InodeState>` objects.
- Directory entries store `BackendInodeId` (or directly store `Arc<InodeState>`); use `Weak` in global tables to allow reclaim when unlinked and no longer open.

### Directory ordering and `read_dir` cursor model

`read_dir(cursor, max)` should keep deterministic behavior:

- If `deterministic_readdir == true`, order by raw name bytes (lexicographic).
- Treat `cursor` as a stable index (cookie) into that ordering for a fixed directory snapshot.
- Document that concurrent directory mutation may cause cursor invalidation or skipping; deterministic behavior is required for tests, not for concurrent mutation fairness.

### Rename semantics and atomicity

- Renames within a single `MemFs` instance are atomic under locks.
- `RenameOptions::noreplace` must be honored.
- `RenameOptions::exchange` can remain `NotSupported` in 6.2 unless implemented with clear semantics.
- Cross-filesystem rename is detected via downcast failure on `new_parent` and must return `VfsErrorKind::CrossDevice`.

### Permission model

`vfs-mem` is the “semantic reference” backend:

- It should store and report mode/uid/gid/timestamps.
- It does not need to implement full permission enforcement at 6.2 unless the VFS layer depends on it.
- If permission enforcement is added, document the exact policy (e.g., always allow, or enforce basic UNIX mode bits).

## Detailed mapping to `vfs-core` traits

### `MemFsProvider` (`FsProviderSync`)

Implement:

- `name()` → `"mem"`
- `capabilities()` → see “Provider capability plan”
- `validate_config(config)`:
  - downcast to `MemFsConfig` and reject other configs with `InvalidInput`
- `mount(req)`:
  - create a new `MemFs` instance (fresh root)
  - store `req.flags` so the FS enforces read-only mode when requested

### `MemFs` (`FsSync`)

Implement:

- `provider_name()` → `"mem"`
- `capabilities()` → see above
- `root()` → returns the root directory node
- `node_by_inode()`:
  - should be implemented once an inode table exists (optional, but recommended because memfs can provide stable inodes cheaply)

### `MemNode` (`FsNodeSync`)

Maintain or implement:

- `lookup`, `create_file`, `mkdir`, `unlink`, `rmdir`, `read_dir`, `rename`, `symlink`, `readlink`, `open`
- `link(existing, new_name)`:
  - support linking regular files (initially)
  - return `NotSupported` for directories and symlinks (until explicitly supported)
  - return `CrossDevice` when `existing` is not a `MemNode` from the same FS instance
- `set_metadata(set)`:
  - enforce read-only mount flag
  - `size` only for regular files; for other types return `InvalidInput` (match current behavior)

### `MemHandle` (`FsHandleSync`)

Implement:

- `read_at`, `write_at`, `set_len`, `len`, `dup`
- enforce read-only mount flag for write operations
- implement `append()` if `vfs-core` OFD layer depends on backend append optimization; otherwise return `Ok(None)` and leave append behavior to the VFS layer

## Testing plan (what to implement in `vfs/mem/tests/`)

Create a focused integration test file, e.g. `vfs/mem/tests/memfs.rs`, covering the acceptance criteria above. Keep tests deterministic and fast; use no external dependencies.

## Documentation requirements

Document explicitly in crate docs or `vfs/docs/spec_6.2.md` updates as the implementation lands:

- What “stable inodes” means for `vfs-mem` (stable within mount lifetime).
- Cursor semantics for `read_dir` and any mutation caveats.
- Read-only mount behavior.
- Any deliberate deviations from strict POSIX (e.g., permission enforcement policy).

