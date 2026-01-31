# Finalize VFS — Implement remaining stubbed functionality (excluding Wasix integration)

## Status / scope

- **Scope**: only `./vfs/*` crates. **Ignore** Wasix integration (Phase 5).
- **Goal**: implement all currently stubbed / incomplete VFS-layer functionality so that the VFS
  crates form a coherent, usable subsystem on their own.
- **Explicitly excluded**: file watching / `FsProviderCapabilities::WATCH` and anything that requires
  Wasix resources/FD tables.

This doc is written as a **junior-executable implementation spec**.

---

## Inventory: what is currently stubbed / incomplete

### A) `vfs-core` facade is ENOSYS

`vfs/core/src/vfs.rs` is a placeholder: every method returns `VfsErrorKind::NotImplemented`:

- `Vfs::openat` / `openat_async`
- `Vfs::statat` / `statat_async`
- `Vfs::mkdirat` / `mkdirat_async`
- `Vfs::unlinkat` / `unlinkat_async`
- `Vfs::renameat` / `renameat_async`
- `Vfs::readlinkat` / `readlinkat_async`
- `Vfs::symlinkat` / `symlinkat_async`
- `Vfs::readdir` / `readdir_async` (see also B)

This is the single biggest “VFS not finished” hole.

### B) Directory streaming API is not actually usable

`vfs-core` defines:

- `DirStreamHandle` (`vfs/core/src/handle.rs`) — currently only wraps an id.
- `Vfs::readdir(...) -> DirStreamHandle` (`vfs/core/src/vfs.rs`) — currently ENOSYS.
- `ReadDirOptions` (`vfs/core/src/flags.rs`) — currently an empty struct.

But there is **no** API to consume a `DirStreamHandle` (no “next batch” / “close” operation).

Meanwhile, the backend traits already support batched listing directly:

- `FsNodeSync::read_dir(cursor, max) -> ReadDirBatch` (`vfs/core/src/traits_sync.rs`)
- `FsNodeAsync::read_dir(cursor, max) -> ReadDirBatch` (`vfs/core/src/traits_async.rs`)

So we must either:

- **(Option 1, recommended)**: implement a small dir-stream abstraction in `Vfs` that turns repeated
  `read_dir(cursor,max)` calls into a stable stream, **or**
- **(Option 2)**: remove/replace the `DirStreamHandle` API and expose directory batching directly at
  the `Vfs` facade level.

This spec chooses **Option 1** (keep existing public shape and make it usable), because it is
additive and minimizes churn to existing docs/specs.

### C) Concrete bug: Windows host `stat_at` ignores `nofollow`

In `vfs/host/src/platform/windows.rs`:

- `stat_at(parent, name, _nofollow)` ignores the boolean argument and always uses
  `symlink_metadata` (i.e. “nofollow” behavior).

This is incorrect with respect to the function signature and will cause mismatched behavior vs Unix
and vs VFS core’s `StatOptions.follow`.

Fixing this is part of “finalize VFS” because it affects `Vfs::statat` correctness on Windows.

---

## Design constraints (must follow)

### 1) Keep semantics in `vfs-core`

Backend providers (`vfs-mem`, `vfs-host`, `vfs-overlay`) implement primitive operations.
**Path semantics** live in:

- `vfs/core/src/path_walker.rs` (`PathWalker` + `PathWalkerAsync`)

**OFD semantics** live in:

- `vfs/core/src/handle.rs` (`VfsHandle` + `VfsHandleAsync`)

The `Vfs` facade should be a “glue” layer:

- resolve path via `PathWalker{,Async}`
- run policy hooks (`ctx.policy`)
- dispatch to `FsNode{,Async}` methods
- wrap results in `VfsHandle{,Async}` / `VfsDirHandle{,Async}`

### 2) No watch work

Do not implement any file watching. Do not add new watch-related APIs.

### 3) Error kinds must be stable

Use existing `VfsErrorKind` values and return consistent errors:

- `NotFound`, `NotDir`, `IsDir`, `AlreadyExists`, `ReadOnlyFs`, `CrossDevice`, `NotSupported`, etc.
- “Not implemented” (`ENOSYS`) should disappear for `Vfs::*` once implemented.

---

## Implementation plan

### Work items (high level)

1. **Implement `vfs-core::Vfs` internals** (`vfs/core/src/vfs.rs`)
2. **Implement usable directory streams** on top of `FsNode{,Async}::read_dir`
3. **Fix Windows host `stat_at(..., nofollow)`** (`vfs/host/src/platform/windows.rs`)
4. **Add/extend tests** in `vfs/core/tests/*` and `vfs/host/tests/*` that exercise the new facade.

---

## 1) Implement `vfs-core::Vfs` (`vfs/core/src/vfs.rs`)

### 1.1 Required internal state

`Vfs` currently contains:

- `struct VfsInner;` (empty)

Replace it with a real inner state:

- **Mount table**: `Arc<MountTable>`
- **Path walkers**: `PathWalker` + `PathWalkerAsync`
  - (they can be constructed from the mount table when needed, but caching is fine)
- **Handle id allocator**: `AtomicU64` for `VfsHandleId`
- **Dir stream table**: a map from `VfsHandleId` → dir stream state (see section 2)

Proposed shape:

```rust
struct VfsInner {
    mount_table: Arc<MountTable>,
    next_handle_id: AtomicU64,
    // dir_streams: Mutex<HashMap<u64, DirStreamState>>
}
```

Add constructor(s) so a caller can build a usable `Vfs`:

- `Vfs::new(mount_table: Arc<MountTable>) -> Self`
  - (this replaces the current `Default`-only empty VFS)
- Keep `Vfs::default()` only if it can build a valid mount table; otherwise remove `Default`
  and require explicit construction.

### 1.2 Mapping `ResolveFlags` → `WalkFlags`

VFS “at” operations accept options with `resolve: ResolveFlags` (`vfs/core/src/flags.rs`).

Implement a helper (private function in `vfs.rs`) to map these into `WalkFlags`:

- `ResolveFlags::NO_SYMLINK_FOLLOW`:
  - set `walk.follow_final_symlink = false`
  - keep `walk.follow_symlinks = true` (intermediate symlinks still followed unless you add a new
    flag; **do not** change semantics without a separate spec)
- `ResolveFlags::BENEATH`:
  - set `walk.resolve_beneath = true`
- `ResolveFlags::IN_ROOT`:
  - set `walk.in_root = true`
- `ResolveFlags::NO_MAGICLINKS`:
  - currently unused by `PathWalker`. Record as “future work”, but do not block this phase.

For `statat`, additionally honor `StatOptions.follow`:

- if `follow == false`, force `walk.follow_final_symlink = false`

### 1.3 Policy hooks

Use `ctx.policy` (`vfs/core/src/policy.rs`):

- `check_open(ctx, node_meta, open_flags)` before opening a file handle
- `check_mutation(ctx, parent_dir_meta, op)` before:
  - `create_file`, `mkdir`, `unlink/rmdir`, `rename`, `link`, `symlink`, `set_metadata`

Notes:

- `PathWalker` already calls `check_path_component_traverse` for each traversed directory component.
- For mutation checks, use the **parent directory’s** metadata.

### 1.4 Rate limiting

`VfsHandle{,Async}` expects a `LimiterChain` (`vfs/core/src/handle.rs`) and `VfsContext` includes
`rate_limiter`.

Required behavior:

- If no limiters are set, pass an empty `LimiterChain`.
- If limiters are set:
  - combine (in any deterministic order) `ctx.rate_limiter`, mount limiter, and fs limiter from the
    mount entry (`vfs/core/src/mount.rs` stores `mount_limiter` and `fs_limiter`).

Implementation approach:

- Get the current mount entry via `mount_table.snapshot()` and `Resolved.mount`.
- Build a `LimiterChain`:
  - include `ctx.rate_limiter` if present
  - include `entry.mount_limiter` if present
  - include `entry.fs_limiter` if present

### 1.5 Implement each `Vfs` method (sync)

All “at” operations share a pattern:

1. Build `WalkFlags` from context + options.
2. Use `PathWalker` to resolve:
   - `resolve(...)` when operating on the target node
   - `resolve_parent(...)` when creating/removing/renaming needs `(parent_dir, name)`
3. Do policy checks.
4. Dispatch to `FsNode` methods.
5. Wrap results.

#### 1.5.1 `statat(ctx, base, path, opts) -> VfsMetadata`

- Resolve with `PathWalker::resolve(...)`.
- If `opts.require_dir_if_trailing_slash` is true:
  - set `walk.must_be_dir = true` OR rely on `PathWalker` trailing-slash behavior; make it explicit
    and add a test.
- Call `resolved.node.metadata()`.
- Return metadata.

#### 1.5.2 `readlinkat(ctx, base, path, opts) -> VfsPathBuf`

- Force `walk.follow_final_symlink = false` so you resolve the symlink itself.
- Resolve and ensure `resolved.node.file_type() == VfsFileType::Symlink` else `InvalidInput` or
  `NotSupported` (pick `InvalidInput` for “readlink on non-symlink”).
- Call `resolved.node.readlink()`.

#### 1.5.3 `mkdirat(ctx, base, path, opts) -> ()`

- Resolve parent: `PathWalker::resolve_parent(...)`.
- Validate `parent.dir.node.file_type() == Directory` (should already be enforced by walker).
- Policy: `check_mutation(ctx, parent_meta, VfsMutationOp::CreateDir)`.
- Call `parent.dir.node.mkdir(name, crate::node::MkdirOptions { mode: opts.mode })`.

#### 1.5.4 `unlinkat(ctx, base, path, opts) -> ()`

- Resolve parent.
- Determine whether this is “unlink file” or “rmdir”:
  - Current `vfs-core` facade `UnlinkOptions` (`vfs/core/src/flags.rs`) does not have `must_be_dir`.
  - Therefore `unlinkat` should behave like “unlinkat(..., 0)” (remove non-directory), and return
    `IsDir` / `NotSupported` as backend dictates when target is a directory.
  - If you need “remove dir” semantics, add a separate facade method `rmdirat` later (not required
    for this spec).
- Policy: `check_mutation(ctx, parent_meta, VfsMutationOp::Remove { is_dir: false })`.
- Call `parent.dir.node.unlink(name, crate::node::UnlinkOptions { must_be_dir: false })`.

#### 1.5.5 `renameat(ctx, base_old, old_path, base_new, new_path, opts) -> ()`

- Resolve old parent + name.
- Resolve new parent + name.
- Enforce same mount if required by semantics:
  - If old and new parents are in different mounts, return `CrossDevice`.
  - (Even if a backend might support cross-device rename, POSIX `rename` does not.)
- Policy:
  - `check_mutation(ctx, old_parent_meta, VfsMutationOp::Rename)`
  - `check_mutation(ctx, new_parent_meta, VfsMutationOp::Rename)` (do both; they can differ)
- Call `old_parent.node.rename(old_name, new_parent.node.as_ref(), new_name, RenameOptions { ... })`
  where the backend `RenameOptions` is `vfs_core::node::RenameOptions`:
  - `noreplace = opts.flags.contains(RENAME_NOREPLACE)`
  - `exchange = opts.flags.contains(RENAME_EXCHANGE)`

#### 1.5.6 `symlinkat(ctx, base, link_path, target, opts) -> ()`

- Resolve parent for `link_path`.
- Policy: `check_mutation(ctx, parent_meta, VfsMutationOp::Symlink)`.
- Call `parent.node.symlink(new_name, target)`.

#### 1.5.7 `openat(ctx, base, path, opts) -> VfsHandle`

Implement the “open file” path. Requirements:

- Resolve with `PathWalker::resolve(...)` if not creating.
- If `opts.flags.contains(OpenFlags::CREATE)`:
  - resolve parent
  - policy: `check_mutation(ctx, parent_meta, VfsMutationOp::CreateFile)`
  - call `parent.node.create_file(name, CreateFile { mode: opts.mode, truncate: flags.contains(TRUNC), exclusive: flags.contains(EXCL) })`
  - then open the returned node (see below)

Opening:

- Fetch node metadata for policy check (`node.metadata()`), then:
  - `ctx.policy.check_open(ctx, &meta, opts.flags)`
- Call `node.open(opts)` to get `Arc<dyn FsHandleSync>`.
- Allocate a new handle id and acquire a `MountGuard` for the resolved mount:
  - `let guard = mount_table.guard(resolved.mount)?;`
- Build `LimiterChain`.
- Return `VfsHandle::new(handle_id, guard, resolved.inode, resolved.node.file_type(), backend_handle, opts.flags, limiter_chain)`.

**Important**: Directory opens.

- With the current API shape, `openat` returns only `VfsHandle`. That handle type is file-I/O centric.
- For now:
  - If `resolved.node.file_type() == Directory`, return `IsDir` (or `NotSupported`), and add a
    dedicated directory-open method in section 2.2.

This is a known limitation of the current facade signatures and is handled by explicitly adding
`opendirat` below.

### 1.6 Implement async variants

Implement the async equivalents using:

- `PathWalkerAsync`
- `FsNodeAsync` methods
- `VfsHandleAsync` / `VfsDirHandleAsync`

The logic should be as close to sync as possible, differing only in `.await` calls and the use of
async metadata/policy inputs.

---

## 2) Make directory handling usable (streams + opening dirs)

### 2.1 Add a minimal dir stream API to `Vfs`

Keep `DirStreamHandle` but make it usable by adding two methods to `vfs/core/src/vfs.rs`:

- `readdir_next(ctx, stream: &DirStreamHandle, max: usize) -> VfsResult<ReadDirBatch>`
- `readdir_close(stream: DirStreamHandle) -> VfsResult<()>`

And async versions:

- `readdir_next_async(...).await -> VfsResult<ReadDirBatch>`
- `readdir_close_async(...) -> VfsResult<()>` (close can be sync)

Dir stream state:

- Store in `VfsInner` a `Mutex<HashMap<u64, DirStreamState>>`.
- `DirStreamState` fields:
  - `dir: VfsDirHandle` (or async dir handle for async streams)
  - `cursor: Option<VfsDirCookie>`
  - `finished: bool`

Behavior:

- `Vfs::readdir(ctx, dir, opts)`:
  - allocate a stream id (use handle id allocator)
  - insert state with `cursor=None`
  - return `DirStreamHandle::new(id)`
- `readdir_next`:
  - look up state by id; if missing: `BadHandle`
  - call `dir.node().read_dir(cursor, max)` and update `cursor = batch.next`
  - return the batch
- `readdir_close`:
  - remove from map

This aligns with `vfs-unix/src/dirents.rs`, which expects a cookie model based on indices.

### 2.2 Add `opendirat` to the facade (required for completeness)

To make the `Vfs` facade self-contained, callers must be able to open a directory by path and obtain
a `VfsDirHandle{,Async}` to use as a base for “at”-style operations and for directory listing.

Add methods (sync + async) to `vfs/core/src/vfs.rs`:

- `opendirat(ctx, base, path, opts: OpenOptions) -> VfsResult<VfsDirHandle>`
- `opendirat_async(ctx, base_async, path, opts: OpenOptions) -> VfsResult<VfsDirHandleAsync>`

Rules:

- Reuse the same `OpenOptions` type so callers can use `ResolveFlags::{BENEATH,IN_ROOT,NO_SYMLINK_FOLLOW}`.
- `OpenOptions.flags` should be validated:
  - Must include `OpenFlags::DIRECTORY` (otherwise `InvalidInput`).
  - Must **not** include file-mutation flags that don’t make sense (`TRUNC`, etc.) → `InvalidInput`.
  - If `WRITE` is requested, treat it as “directory mutations may be allowed”:
    - still return a `VfsDirHandle` (directory handles don’t perform file I/O)
    - policy checks are enforced at mutation time (mkdir/unlink/rename/etc.), not at `opendirat`

Implementation:

- Resolve final node via `PathWalker::resolve` with:
  - `walk.must_be_dir = true`
  - `walk.follow_final_symlink` driven by resolve flags (and `NO_SYMLINK_FOLLOW`)
- Acquire `MountGuard` for the resolved mount.
- Allocate a `VfsHandleId`.
- Construct a `VfsDirHandle::new(id, guard, resolved.inode, resolved.node.clone(), resolved.parent.map(...))`.
  - Use `resolved.parent` when available to preserve mount-root `..` behavior for handles (see
    `PathWalker` tests around detached mounts).

### 2.3 Decide what `Vfs::openat` does for directory targets

After adding `opendirat`, keep `openat` behavior simple:

- If target is a directory, return `IsDir`.
- If caller wants a directory handle, they must call `opendirat`.

This avoids overloading `VfsHandle` with directory semantics and keeps the API explicit.

---

## 3) Fix Windows host backend `stat_at(..., nofollow)`

File: `vfs/host/src/platform/windows.rs`

Current behavior:

- `stat_at(parent, name, _nofollow)` always calls `stat_path(..., follow=false)` (i.e. always uses
  `symlink_metadata`).

Required behavior:

- If `nofollow == true`:
  - use `std::fs::symlink_metadata` (do not follow)
- If `nofollow == false`:
  - use `std::fs::metadata` (follow)

Implementation detail:

- Update `stat_at` to call `stat_path(&path, follow)` where `follow = !nofollow`.

Add a Windows-only unit test (or cfg-gated test) that verifies the boolean is plumbed correctly.
If creating symlinks is not feasible in CI on Windows, test with a plain file and at least assert
the “follow=true” branch is used (e.g. by splitting `stat_path` into two internal helpers and
unit-testing the branch selection).

---

## 4) Tests (minimum required to call this “finalized”)

### 4.1 New `vfs-core` tests for the `Vfs` facade

Add a new test module file:

- `vfs/core/tests/vfs_facade.rs`

Use an in-memory backend (`vfs-mem`) and/or a minimal test backend to avoid touching the host FS.

Test cases (sync):

1. **`statat` basic**
   - mount a memfs as root in a `MountTable`
   - create `/dir/file`
   - `statat(Cwd, "dir/file", follow=true)` returns `RegularFile`
2. **`openat` + OFD handle semantics**
   - open a file for write, write bytes, seek, read back (through `VfsHandle` methods)
3. **`mkdirat` + `unlinkat`**
   - `mkdirat("a")` then `unlinkat("a")` should fail (directory) with a stable error (`IsDir` or backend-specific)
   - `mkdirat("a")` then create `a/file` then `unlinkat("a/file")` succeeds
4. **`renameat`**
   - create `a/file`, rename to `b/file2` and validate lookup via `statat`
5. **`readlinkat` + `symlinkat`**
   - if memfs supports symlinks (or use a backend that does), create symlink and read it back
6. **`opendirat` returns a `VfsDirHandle`**
   - open `/dir` via `opendirat`
   - call `dir.node().read_dir(None, ...)` directly and ensure entries are present

Test cases (async):

Add `vfs/core/tests/vfs_facade_async.rs` (or `#[tokio::test]` gated behind existing async test setup):

- Mirror 2–3 key sync tests for async variants (`openat_async`, `statat_async`, `mkdirat_async`).

### 4.2 New tests for dir streams (if Option 1 is implemented)

Add tests that validate:

- `readdir` returns a handle
- `readdir_next` returns batches and advances cursor
- `readdir_close` invalidates the handle (further `readdir_next` returns `BadHandle`)

### 4.3 Regression test for Windows `nofollow` plumb

See section 3.

---

## 5) Acceptance criteria checklist

This phase is complete when:

- `vfs/core/src/vfs.rs` contains real implementations (no `NotImplemented` errors) for:
  - `openat{,_async}`, `statat{,_async}`, `mkdirat{,_async}`, `unlinkat{,_async}`,
    `renameat{,_async}`, `readlinkat{,_async}`, `symlinkat{,_async}`
- `Vfs` can be constructed with a real `MountTable` and is usable without Wasix.
- Directory operations are possible end-to-end:
  - `opendirat` exists and returns `VfsDirHandle{,Async}`
  - `readdir` is no longer ENOSYS and has a way to retrieve entries (`readdir_next`), or the API is
    otherwise made coherent (but do not leave `DirStreamHandle` unusable).
- Windows host backend `stat_at` honors `nofollow`.
- `cargo test -p vfs-core` passes, including the new facade tests.

---

## 6) Non-goals / future work (explicit)

- File watching API and implementations.
- `ResolveFlags::NO_MAGICLINKS` semantics (can be added later, likely requiring `openat2` on Linux).
- Cross-device rename/link (keep returning `CrossDevice`).
- Richer dir stream options (`ReadDirOptions` filtering, ordering guarantees beyond backend contract).

