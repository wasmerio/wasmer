# Spec 6.5 — Overlay + union provider (`vfs-overlay`)

## Status

- **Target plan section**: `fs-refactor.md` → **Phase 6.5 Overlay + union provider**
- **This spec assumes**:
  - **Phase 4 is complete**: sync/async traits and adapters exist in `vfs-core` (overlay can be sync-only and still be usable from async via adapters).
  - **Mount flags exist** (`vfs_core::provider::MountFlags`) and are forwarded by the caller at mount time.
  - We **must not change the existing `vfs-core` public API surface** (traits, structs, type names). Any missing behavior must be implemented inside `vfs-overlay`.

## Summary (one paragraph)

Implement `vfs-overlay` as a registry-mountable provider named **`"overlay"`** that exposes a merged view of **one writable upper filesystem** plus **one-or-more lower filesystems**. The merged view must support Linux-like overlay semantics required by Wasix rootfs layering: **upper shadows lower**, directories are **union-merged**, and deletions of lower-only entries are represented via **whiteouts** stored in the upper. Mutations to lower-only entries must trigger **copy-up into upper** (preferably at `open()` time when write intent is present). “Opaque directories” must be supported to hide lower entries beneath a directory. The implementation must be safe-by-default by reserving and hiding internal overlay metadata names from the merged namespace.

## Current implementation baseline (what exists today)

This repository already contains a functional `vfs-overlay` implementation:

- **Provider and config**:
  - `vfs/overlay/src/config.rs` defines:
    - `OverlayProvider` implementing `vfs_core::provider::FsProvider` (alias of `FsProviderSync`)
    - `OverlayConfig` with nested `FsSpec { provider, config }` for upper and lowers
    - `OverlayBuilder` for building `OverlayFs` directly (bypassing provider registry)
    - `OverlayOptions` controlling copy-up chunking and reserved-name policy
- **Filesystem implementation**:
  - `vfs/overlay/src/fs.rs` defines `OverlayFs: vfs_core::Fs` (alias of `FsSync`).
  - `vfs/overlay/src/node.rs` defines `OverlayNode: vfs_core::node::FsNode` (alias of `FsNodeSync`).
  - `vfs/overlay/src/handle.rs` defines `OverlayHandle` wrapping the underlying handle.
- **Overlay semantics**:
  - Upper shadowing, merged `readdir`, whiteouts, opaque dirs, copy-up-on-open-for-write, and rename/link constraints are implemented.
  - Whiteouts + opaque markers are represented as **reserved files** in upper:
    - reserved prefix: `.wasmer_overlay.`
    - opaque marker: `.wasmer_overlay.opaque`
    - whiteout marker prefix: `.wasmer_overlay.wh.<name>`
  - Reserved names are hidden from `lookup()`/`readdir()` and can be denied on create.
- **Inode stability**:
  - `vfs/overlay/src/inodes.rs` contains `OverlayInodeTable` that maps backing (upper/lower) inode keys to a stable overlay inode id, and “promotes” to the upper key after copy-up.
- **Tests**:
  - `vfs/overlay/src/tests/mod.rs` covers key behaviors (shadowing, merged readdir, whiteouts, opaque marker, copy-up, rename constraints, inode stability).

This spec therefore focuses on (a) documenting the contract precisely for future contributors and (b) identifying remaining gaps and acceptance tests required for Phase 6.5 “done”.

## Non-goals (explicitly out of scope for 6.5)

- **Full Linux overlayfs parity**: redirect_dir, index, metacopy, xino, NFS export, and every corner-case around open handles during rename are not required.
- **Persistent overlay metadata across providers** beyond storing markers as normal files in the upper (no xattrs/ACL requirements).
- **Watch API integration**: `vfs-core` does not define watch traits yet, so `vfs-overlay` must not claim watch capability.
- **Supporting user-visible creation of reserved metadata names**. The `.wasmer_overlay.*` namespace remains internal.
- **Overlay-of-overlay recursion**: 6.5 should not require mounting an overlay whose upper/lower layers are themselves created by the overlay provider. (If desired later, we can flatten layers instead.)

## Goals and acceptance criteria

### Functional goals

- **Mountable via registry**:
  - Provider name is exactly `"overlay"`.
  - `FsProviderRegistry::register(OverlayProvider)` works.
  - `FsProviderRegistry::mount_with_provider("overlay", OverlayConfig, …)` returns an `Fs` whose root behaves as an overlay mount.
- **Correct overlay read semantics**:
  - Upper shadows lower for same name.
  - `read_dir` returns merged view:
    - upper entries first, excluding internal markers
    - then lower entries not shadowed by upper and not whiteouted
    - if a directory is marked opaque, it hides lower entries entirely under that directory
- **Correct overlay write semantics**:
  - Any mutation goes to upper only.
  - Mutating a lower-only entry must **copy-up** to upper first.
  - Lower layers must never be modified by overlay operations (unless we explicitly add a “writable-lower” mode in the future).
- **Whiteouts + opaque directories**:
  - Unlinking / rmdir on a lower-only entry creates a whiteout marker in upper.
  - Opaque directory marker in upper hides lower entries.
- **Stable inode ids across copy-up**:
  - A node’s `inode()` must remain stable across copy-up (at least for the lifetime of the overlay fs instance).

### Error semantics goals

- Cross-filesystem operations must fail fast:
  - `rename` and `link` must return `VfsErrorKind::CrossDevice` when the other node is not from the same overlay instance.
- `RenameOptions::exchange` must return `VfsErrorKind::NotSupported`.
- Renaming a lower-only directory must return `VfsErrorKind::NotSupported` (v1 limitation; must be tested).
- Reserved internal names are **not visible** in overlay namespace:
  - `lookup()` on reserved names returns `NotFound` (even if present as overlay metadata)
  - `readdir` never returns reserved names
  - if configured (`deny_reserved_names=true`), attempts to create them return `PermissionDenied`

## Public API and configuration

### Crate exports

`vfs-overlay` should export:

- `OverlayProvider` — registry-visible provider (`FsProviderSync`)
- `OverlayConfig` — mount configuration with nested provider specs
- `OverlayBuilder` — direct constructor for `OverlayFs` (useful for tests)
- `OverlayOptions` — overlay behavior knobs (copy-up chunk sizing, reserved-name policy)
- `OverlayFs` — the filesystem instance (rarely referenced directly by callers)

### `OverlayConfig` schema (registry mount)

`OverlayConfig` must be used as the `ProviderConfig` for the provider:

- **`upper: FsSpec`**:
  - `provider: String` (e.g. `"mem"`, `"host"`)
  - `config: Box<dyn ProviderConfig>` (provider-specific)
- **`lowers: Vec<FsSpec>`**:
  - one or more entries
- **`options: OverlayOptions`**

Validation rules (`OverlayProvider::validate_config`):

- Config must downcast to `OverlayConfig`, else `InvalidInput`.
- `lowers` must be non-empty, else `InvalidInput`.
- (Recommended) reject empty provider names in `FsSpec` entries to surface clearer errors early.

### Overlay metadata storage scheme

Overlay-internal metadata is stored in the **upper filesystem** using reserved names:

- **Opaque directory marker**: a file named `.wasmer_overlay.opaque` in the directory.
- **Whiteout marker**: a file named `.wasmer_overlay.wh.<original_name>` in the directory.
- **Reserved prefix** `.wasmer_overlay.` must be:
  - filtered out of `lookup` and `read_dir`
  - optionally rejected on create via `OverlayOptions::deny_reserved_names`

Rationale:

- Works across all backends without needing xattrs.
- Keeps overlay semantics implementable with the current `vfs-core` trait surface.
- Avoids requiring “Linux overlayfs char-device whiteouts”.

## Overlay semantics (detailed)

### Lookup and shadowing

`OverlayNode::lookup(name)` must resolve children as follows:

1. **Reserved name**:
   - If `name` is reserved (`.wasmer_overlay.*`), return `NotFound`.
2. **Upper lookup**:
   - If upper exists and contains `name`, return an overlay child node backed by that upper node.
   - If the child is a directory and the parent is not opaque, also collect lower directory nodes for merging.
3. **Whiteout check**:
   - If upper exists and contains a whiteout marker for `name`, return `NotFound`.
4. **Opaque directory rule**:
   - If the parent directory is opaque, do not consult lowers; return `NotFound`.
5. **Lower lookup (first match wins)**:
   - Search lowers in order and return the first match as a lower-backed overlay node.

### `read_dir` merge rules

For an overlay directory:

- Start with an empty `seen` set and `whiteouts` set.
- Read all entries from upper:
  - Skip reserved names.
  - Detect and remove opaque marker from output; remember `opaque=true` if present.
  - Detect whiteout markers:
    - add the stripped name into `whiteouts`
    - do not return the marker as a directory entry
  - For normal names:
    - mark as seen
    - return directory entry as viewed through overlay (inode + type from overlay lookup)
- If `opaque=true`, stop (do not read lowers).
- Otherwise, for each lower in order:
  - add entries not in `seen` and not in `whiteouts`
  - return directory entries as viewed through overlay

Cursor model:

- Implement `cursor` as an index into the merged list (cookie = `end`).
- It is acceptable (v1) that the implementation materializes the full merged list for each call and slices it.

### Whiteouts

When an entry exists in lower but needs to be hidden from the merged view (because it was deleted, or because a rename replaced it), `vfs-overlay` must:

- Ensure an upper directory exists for the parent (`ensure_upper_dir`)
- Create a whiteout marker file:
  - `.wasmer_overlay.wh.<name>`
  - created as an empty (or truncated) regular file

### Opaque directories

Opaque marker behavior:

- If a directory contains `.wasmer_overlay.opaque` in upper:
  - the merged view treats that directory as containing only upper entries (minus markers)
  - all lower entries beneath that directory are invisible

Who creates opaque markers?

- v1: they can be created explicitly (e.g., by tooling), and the overlay must honor them.
- (Optional later) overlay may auto-mark directories opaque during certain copy-up scenarios; this is not required for 6.5.

### Copy-up semantics

Copy-up must occur **before the first mutation** of a lower-only entry.

Required v1 behavior:

- **Copy-up on `open()` with write intent** (preferred):
  - If `OpenOptions.flags` includes any of:
    - `WRITE`, `TRUNC`, `CREATE`, `APPEND`
  - then:
    - regular file: copy file data + metadata into upper (`copy_up_file`)
    - symlink: copy symlink target into upper (`copy_up_symlink`)
    - directory: do not copy-up on open; directory mutation paths must ensure `ensure_upper_dir()`
- Copy-up functions must:
  - create the destination in upper with a best-effort mode derived from lower metadata
  - copy bytes in chunks (bounded by `OverlayOptions::max_copy_chunk_size`)
  - apply metadata best-effort (`set_metadata`)

### Rename semantics

Rename is implemented via upper-only operations, with overlay consistency maintained by creating whiteouts when needed:

- If `opts.exchange=true`: return `NotSupported`.
- If `opts.noreplace=true` and destination exists in overlay view: return `AlreadyExists`.
- If source is lower-only directory: return `NotSupported` (v1 limitation).
- If source is lower-only file or symlink: copy-up first.
- Perform `rename` in upper from src parent upper dir to dst parent upper dir.
- If source was lower-only: create a whiteout in source parent for `old_name`.
- If destination existed in lower but not in upper: create a whiteout in destination parent for `new_name`.

### Hardlinks and symlinks

- `link(existing, new_name)`:
  - Must ensure `existing` is copied up into upper first (hardlinks operate within upper).
  - Must return `NotSupported` for directories (v1).
- `symlink(new_name, target)`:
  - Creates symlink in upper parent directory.
- `readlink()`:
  - Delegates to the active node (upper if present, otherwise lower).

## Provider capability reporting

### `FsProviderCapabilities` (registry-facing)

`OverlayProvider::capabilities()` should be conservative:

- At minimum, it should not claim capabilities that depend on the underlying layers unless it validates them at mount time.
- Recommended v1 behavior:
  - return `FsProviderCapabilities::empty()` (current implementation), and rely on `Fs::capabilities()` for coarse gating.

### `VfsCapabilities` (Fs-instance-facing)

`OverlayFs::capabilities()` should reflect what the overlay instance can do.

Recommended v1 approach:

- Start with `upper.capabilities()` as the baseline (writes happen there).
- Optionally, **mask out** capabilities that lower layers cannot support for read semantics if the VFS core depends on them (e.g. if `readlink` is required for symlink resolution, ensure lowers support it or return `NotSupported` appropriately).

In 6.5, it is acceptable to:

- use `upper.capabilities()` and rely on operation-level errors for corner cases,
- but the behavior must be documented and tested for at least symlinks and hardlinks.

## Deliverables (files and modules)

`vfs/overlay/src/` should keep its current modular structure:

- `lib.rs` — exports
- `config.rs` — provider + config + builder
- `fs.rs` — `OverlayFs`
- `node.rs` — `OverlayNode` and the core overlay logic
- `copy_up.rs` — copy-up helpers (file/dir/symlink)
- `whiteout.rs` — reserved-name + marker helpers
- `inodes.rs` — stable inode mapping across layers
- `handle.rs` — handle wrapper
- `tests/` — functional tests

## Implementation plan (step-by-step, junior-friendly)

Even though the bulk of overlay semantics are already implemented, Phase 6.5 should be considered “done” only after the following tasks are complete.

### 1) Registry integration tests (missing coverage)

Add `vfs/overlay/tests/provider_mount.rs` that:

- creates an `FsProviderRegistry`
- registers:
  - `vfs_mem::MemFsProvider` (or equivalent provider in tree)
  - `vfs_overlay::OverlayProvider`
- mounts `"overlay"` using an `OverlayConfig` whose upper+lower are `mem`
- asserts the overlay behavior via the mounted `Fs`

Goal: prove that the provider-based mount path works end-to-end (not just `OverlayBuilder`).

### 2) Reserved-name policy tests

Add tests for:

- `lookup(".wasmer_overlay.opaque")` is `NotFound` (even if present)
- `readdir` never lists `.wasmer_overlay.*`
- with `OverlayOptions { deny_reserved_names: true }`, attempts to create a reserved name return `PermissionDenied`

### 3) Multi-lower layering tests

Add a test with 2+ lowers proving:

- lookup prefers the first lower that contains the name (after upper/whiteout/opaque rules)
- `readdir` returns:
  - upper entries first
  - then entries from lower[0], then lower[1] (excluding shadows and whiteouts)

### 4) Read-only mount behavior (document + test)

Clarify and test what happens when the overlay mount is created with `MountFlags::READ_ONLY`.

Minimum required behavior:

- All mutating operations via overlay should fail (ideally with `ReadOnlyFs`).

If underlying upper provider already enforces this, tests should demonstrate it; if not, implement explicit gating in overlay (store the mount flags in `OverlayFs` and check them before any mutation/copy-up).

### 5) Capability consistency checks (document)

Document (and optionally test) how overlay capabilities are computed:

- `OverlayFs::capabilities()` currently forwards `upper.capabilities()`.
- If this is kept, add a short rationale and a list of known mismatches (if any).

## Testing plan (minimum “green bar” for 6.5)

- `cargo test -p vfs-overlay`
- Tests must be deterministic and not depend on host filesystem ordering.

## Follow-ups (explicitly not required for 6.5, but expected later)

- **Performance**: avoid reading entire directories on every `read_dir` call by caching merged dir listings per `OverlayNode` and invalidating on mutations.
- **Metacopy**: metadata-only copy-up for chmod/chown/utimens without copying file data.
- **Better provider capabilities**: compute a meaningful intersection/union of upper+lower features and expose it via `FsProviderCapabilities`.
- **Overlay export/import tooling**: optional utilities to materialize overlays or convert between marker schemes, if needed for packaging.

