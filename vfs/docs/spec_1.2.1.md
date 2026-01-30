
Below are **full implementation specs** (not stubs) for each file, written so an engineer can implement them with minimal ambiguity while staying consistent with **every requirement in the overall plan** (Linux-like semantics, inode-driven mounts, OFD correctness, at-style ops, capability truthfulness, performance, modularity, sync+async compatibility, and later overlay/hostfs constraints).

---

# `vfs/core/src/path_walker.rs` — PathWalker (full)

## Responsibilities

`PathWalker` is the **single source of truth** for namespace traversal semantics:

* Splitting and iterating `VfsPath` components.
* Applying **`.` / `..`**, **trailing slash** rules, and **symlink resolution**.
* Performing **inode-driven mount transitions** (enter mount, `..` at mount root).
* Supporting **at-style resolution**: base directory handle + relative path (no string prefix hacks).
* Returning a **resolved target** plus useful intermediates for ops (e.g., parent dir + final name).

This module must not contain backend-specific logic (no `std::fs`, no Windows handling). It uses the backend via `FsNode` APIs (from `node.rs`) and mount table APIs (from `mount.rs`).

## Key public types

### `ResolveFlags` / `WalkFlags`

Use the internal flags from `flags.rs` but define a dedicated struct for walking:

* `follow_symlinks: bool` (default true; overridden by open/stat options)
* `follow_final_symlink: bool` (for lstat-like)
* `must_be_dir: bool` (derived from trailing slash or open DIRECTORY flag)
* `allow_empty_path: bool` (for special cases; generally false)
* `max_symlinks: u16` (from `VfsConfig`)
* `resolve_beneath: bool` / `in_root: bool` (reserve for future; implement semantics only if already planned—otherwise keep “unsupported” error early but wire the flag)

### `Resolved`

Returned from main resolution routines.

* `mount: MountId`
* `inode: VfsInodeId`
* `node: Arc<dyn FsNode>` (or an internal `NodeRef` that carries mount+node)
* `parent: Option<ResolvedParent>` (when caller requested parent+name)
* `traversal: TraversalInfo` (optional, behind feature or debug; counters only)

`ResolvedParent`:

* `dir: Resolved` (directory containing the final component)
* `name: VfsNameBuf` (final component name bytes)
* `had_trailing_slash: bool`

### `ResolutionRequest`

* `ctx: &VfsContext`
* `base: VfsBaseDir` (Cwd or directory handle)
* `path: &VfsPath`
* `flags: WalkFlags`

## Core APIs

### `resolve(&self, req) -> VfsResult<Resolved>`

Resolves path to a final node subject to flags.

### `resolve_parent(&self, req) -> VfsResult<ResolvedParent>`

Resolves all but final component. Used for create/unlink/rename/link etc.

### `resolve_at_component_boundary(...)`

Internal helper used by overlay and rename flows: resolves a path but allows caller to intercept when reaching the last component (e.g., for `O_CREAT|O_EXCL` checks).

## Semantics and edge cases (must be tested)

### 1) Absolute vs relative

* If `path.is_absolute()`: start at VFS namespace root (mount table’s root mount + its root inode).
* Else: start at `base`:

  * `Cwd` ⇒ `ctx.cwd` directory handle/inode.
  * `Handle(dir)` ⇒ that directory.

### 2) Empty path

* Default: `InvalidInput` (maps to `EINVAL` later) unless operation explicitly allows it.
* Some WASI calls treat empty path as “operate on base” — if Wasix needs that, expose `allow_empty_path` and make it opt-in per syscall mapping.

### 3) Repeated slashes

* Treated as a single separator for component iteration.
* Trailing slash preserved via `VfsPath::has_trailing_slash()` and influences `must_be_dir`.

### 4) `.` and `..`

* `.`: no-op.
* `..`: Linux-like mount boundary behavior:

  * If current inode is the **root of a mounted filesystem**: transition to parent mount and set inode to mountpoint inode (mount table provides this).
  * Else: resolve parent via backend `parent()`/`lookup("..")` semantics are *not* used; the VFS must use backend directory traversal:

    * Prefer backend-provided `lookup(parent_dir, "..")`? No—this is unsafe/ambiguous across backends.
    * Instead, require each `FsNode` directory to support `lookup(parent_name)` by name, not by `..`.
    * So VFS maintains its own current directory and uses `..` by:

      * calling backend `dir_parent()` if the backend supports it **and** it is capability-truthful, or
      * maintaining a traversal stack of `(dir inode, parent inode)` as it walks (recommended for correctness and cross-backend consistency).
  * **Spec choice (recommended): traversal stack**

    * During walking, keep stack of visited directories with their inodes.
    * On `..`, if stack has previous entry and not at mount root, pop.
    * This matches typical path-walk implementations and avoids relying on backend `..` semantics.
    * It also makes mount-root handling straightforward.
  * Special case: at namespace root, `..` stays at root.

### 5) Symlink resolution

Symlink rules must match POSIX-ish + Linux expectations:

* If encountering a symlink in a **non-final** component: always follow (unless `resolve_beneath` semantics would disallow; then return `PermissionDenied`/`InvalidInput` per your eventual policy).
* If symlink is the **final** component:

  * Follow if `follow_final_symlink == true`
  * Do not follow if false (lstat / NOFOLLOW)
* Each symlink follow increments a counter; if exceeds `max_symlinks`, return `TooManySymlinks`.
* Reading a symlink uses backend `readlink()` which returns a `VfsPathBuf` (raw bytes).
* When following a symlink target:

  * If target is absolute: restart from namespace root.
  * If relative: interpret relative to directory containing the symlink.
  * Preserve remaining unresolved components by constructing an internal “worklist” of components. Do **not** allocate full strings in a loop—use a small VecDeque of slices / `VfsPathBuf` scratch.
* Trailing slash + symlink: if final result must be dir and following resolves to non-dir ⇒ `NotDir`.

### 6) Mount transitions (inode-driven)

At each successful component step that yields a directory entry inode:

* Check mount table: `mount_table.enter_if_mountpoint(current_mount, child_inode)`
* If yes:

  * set `current_mount = mounted_mount_id`
  * set `current_inode = mounted_root_inode`
  * set `current_node = fs.root_node()` (or cached root node handle)
    This must be a single O(1) check (hash lookup keyed by mountpoint inode).

### 7) Trailing slash

If input path has trailing slash OR caller passes `must_be_dir`:

* The resolved final node must be a directory.
* If final component is missing: return `NotFound`.
* If final exists but not dir: return `NotDir`.

### 8) Race behavior

Core is userspace; we do not promise perfect race-free resolution across hostile mutable host fs, but:

* Hostfs must enforce confinement via openat/handles.
* PathWalker must avoid “string prefix” confinement logic.
* PathWalker should be structured so hostfs can implement “walk using dir handles” later without changing semantics:

  * i.e., every step is `(dir node, name) -> child node`.

## Performance constraints

* No per-component allocations on the hot path.
* Avoid building whole normalized strings.
* Use small stack-allocated buffers where possible (e.g., `SmallVec<[Component; 16]>` for typical paths).
* Allow mount table snapshot to be read without locks (ArcSwap snapshot model).

## Required tests (in `vfs-core/tests/path_walker.rs`)

* Absolute vs relative; base handle.
* `..` across mount roots.
* Symlink depth loop detection.
* Final symlink NOFOLLOW vs follow.
* Trailing slash errors.
* Cross-mount behavior: entering mount changes fs root.

---

# `vfs/core/src/mount.rs` — Mount table (full)

## Responsibilities

* Maintain a Linux-like mount namespace mapping.
* Provide inode-driven mount transitions and `..` mount-root behavior.
* Support mount/unmount with busy detection and optional lazy detach.
* Serve as a fast, read-mostly structure used by PathWalker.

## Core design (must match plan)

* Reads are hot, mount changes are rare.
* Use **immutable snapshots** behind `ArcSwap` (or equivalent) so:

  * Path resolution holds `Arc<MountTableInner>` snapshot while walking.
  * Mount/unmount replaces snapshot atomically.

## Types

### `MountTable`

* holds `ArcSwap<MountTableInner>` (or internal RW lock if ArcSwap not allowed, but performance target says snapshot swap)

### `MountTableInner`

* `root: MountId`
* `mounts: Slab<MountEntry>` or `Vec<MountEntry>` indexed by MountId (MountId is NonZeroU32 recommended)
* `mount_by_mountpoint: HashMap<VfsInodeId, MountId>`

  * key is the **mountpoint inode in parent mount**
* `children_by_parent: HashMap<MountId, SmallVec<[MountId; 4]>>` (optional; useful for unmount recursion / diagnostics)

### `MountEntry`

* `id: MountId`
* `parent: Option<MountId>` (None for root)
* `mountpoint: Option<VfsInodeId>` (None for root mount)
* `root_inode: VfsInodeId` (root node inside this mounted fs)
* `fs: Arc<dyn Fs>` (provider instance; actual trait from provider layer)
* `flags: MountFlags`
* `state: MountState`

  * `Active`
  * `Detached` (lazy unmount)
* `open_count: AtomicU64` (or handle to a shared refcount object) for busy checks without global locks

### `MountFlags`

* `READ_ONLY`
* `NOEXEC`, `NOSUID`, etc (optional; policy may handle)
* `DETACHABLE` (informational)

## APIs

### Read-path APIs (hot)

* `snapshot(&self) -> Arc<MountTableInner>`
* `enter_if_mountpoint(inner, current_mount: MountId, child: VfsInodeId) -> Option<MountId>`

  * lookup in `mount_by_mountpoint`
* `mount_root(inner, mount: MountId) -> (VfsInodeId, Arc<dyn Fs>)`
* `parent_of_mount_root(inner, mount: MountId) -> Option<(MountId, VfsInodeId)>`

  * returns `(parent_mount, mountpoint_inode)` for `..` logic

### Update APIs (cold)

* `mount(&self, parent_mount: MountId, mountpoint_inode: VfsInodeId, fs: Arc<dyn Fs>, root_inode: VfsInodeId, flags: MountFlags) -> VfsResult<MountId>`

  * Fails if mountpoint already has a mount.
  * Fails if mountpoint inode not a directory? (should be enforced by caller/pathwalker; but mount can double-check via metadata)
* `unmount(&self, target_mount: MountId, flags: UnmountFlags) -> VfsResult<()>`

  * If `open_count > 0`:

    * default: `Busy`
    * if `MNT_DETACH`: mark `Detached` and remove from namespace mapping but keep entry alive until open_count hits 0.
  * Removing from namespace means:

    * delete from `mount_by_mountpoint`
    * remove from children index
    * keep `MountEntry` available for handles that already reference it.

### Busy tracking

* `MountGuard` acquired when creating `VfsHandle`/watcher on a mount:

  * increments `open_count` on mount entry
  * decrements on drop
* `VfsHandle` must hold a `MountGuard` to keep mount alive during lazy unmount.

## Correctness rules

* Mount transitions are **inode-based**, not string prefix-based.
* `..` across mount boundary uses `parent_of_mount_root`.
* Cross-mount rename checks: if source mount != dest mount ⇒ `CrossDevice`.

## Tests

* Mount nesting.
* Enter mount during traversal.
* `..` at mount root returns mountpoint inode in parent.
* Busy unmount returns Busy.
* Lazy detach removes from traversal but existing handles still work until close.

---

# `vfs/core/src/handle.rs` — OFD handle types (full)

## Responsibilities

Implement Linux-like **Open File Description** semantics:

* Shared offset across dup’d FDs.
* Status flags that are shared vs per-FD flags split (CLOEXEC stays outside core).
* Typed I/O surface for sync+async usage (with adapters later).
* Correct semantics for `O_APPEND`, `O_TRUNC`, `O_NONBLOCK` (as possible).
* Maintain mount lifetime (lazy unmount correctness).

## Split of responsibilities

* `VfsHandle` is the OFD-like object.
* Wasix resource table holds per-FD flags and references `Arc<VfsHandle>`.

## Types

### `VfsHandle`

* `id: VfsHandleId`
* `mount_guard: MountGuard`
* `inode: VfsInodeId`
* `file_type: VfsFileType` (cached)
* `state: Arc<OFDState>`
* `inner: Arc<dyn FsHandle>` (backend handle trait object)

### `OFDState`

Shared among dup’d handles:

* `offset: AtomicU64` or a lock (see below)
* `status_flags: AtomicU32` (APPEND, NONBLOCK, SYNC…)
* `append: bool` (derivable from status_flags)
* Optional: `dir_pos` for directory streams if you unify; or keep separate `VfsDirStream`.

**Offset concurrency choice**

* For correctness under concurrent reads/writes, offset updates must be atomic with the I/O op.
* Recommended:

  * store offset in a `parking_lot::Mutex<u64>` so you can:

    * lock, read offset, do I/O, update offset, unlock.
  * This avoids subtle races where atomic fetch_add doesn’t match actual bytes read/written.
* Provide `pread/pwrite` style methods that do not touch offset.

### `FsHandle` backend trait (conceptual; defined in `node.rs`/provider traits)

Must support:

* `read_at(off, buf)`, `write_at(off, buf)`
* `flush`, `fsync`
* `set_len`, `get_len` (optional)
* `get_metadata` maybe
* directory handles: separate trait/type is cleaner (`FsDirHandle`) rather than overloading.

## API surface on `VfsHandle`

Sync methods (async equivalents later via trait set or adapters):

* `read(&self, buf: &mut [u8]) -> VfsResult<usize>`

  * uses shared offset, updates it
* `write(&self, buf: &[u8]) -> VfsResult<usize>`

  * if APPEND: perform atomic append semantics *as best as backend allows*

    * If backend supports `append_write` or `write_at(EOF)` atomically, use it.
    * Else do: lock offset? For append, offset isn’t source of truth; must query size and write. This can race; if backend cannot guarantee atomic append, capability must reflect that and either:

      * allow best-effort with documented limitation, or
      * return NotSupported when strict append required (you probably want best-effort for hostfs/memfs; object store likely NotSupported).
* `seek(&self, whence, offset) -> VfsResult<u64>`

  * updates shared offset
* `pread/pwrite` do not update offset.
* `set_status_flags`, `get_status_flags` (shared flags)

Dup semantics:

* `fn dup(&self) -> Arc<VfsHandle>` returns a new handle object sharing `state` and `inner` as appropriate.

  * If backend handle can be dup’d cheaply, you may keep same inner and rely on OFDState offset; but some backends need per-handle resources. Provide `FsHandle::dup()` optional; if not supported, wrap inner in Arc + internal mutex.

Close semantics:

* Drop decrements mount_guard and backend handle drops.

## Tests

* `dup` shares offset: write then dup then read and offsets match.
* `pread` doesn’t affect offset.
* `O_APPEND` writes at end despite current offset.
* `O_TRUNC` occurs at open time (handled in open path, but test via open options integration).

---

# `vfs/core/src/inode.rs` — inode semantics (full)

## Responsibilities

* Define inode identity and stability rules.
* Provide inode caching and lookup hooks needed by mount table and path walker.
* Make overlay/object-store feasible by supporting synthetic inode IDs with a stability contract.

## Core types (from `ids.rs`)

* `MountId`
* `BackendInodeId: u64`
* `VfsInodeId { mount: MountId, ino: BackendInodeId }`

## Inode stability contract (must be enforced/documented)

* Inode identity must be stable **for the lifetime of a mount**.
* It does not need to persist across process restarts.
* For backends without native inodes:

  * backend must maintain a mapping table from stable keys → `BackendInodeId`
  * collisions must be impossible (no “hash-as-id” without collision handling)

## Inode cache (core-level)

Core should provide an optional per-mount inode cache that maps `BackendInodeId` to a weak node reference to reduce allocations:

### `InodeCache`

* keyed by `VfsInodeId`
* values: `Weak<NodeRefInner>` (where NodeRef wraps backend node + mount)
* bounded (LRU) optional; default unbounded for memfs tests, bounded for production.

**Important**: core must not assume that caching a node handle is always valid across backend changes; caching is best-effort. If backend invalidates nodes, operations can return `NotFound` or `StaleHandle` (if you add that kind).

## Metadata caching

Do **not** bake metadata caching into inode.rs yet unless you already need it. But reserve:

* `VfsMetadata` includes `inode`, `generation` optional, `device` optional.
* Later network/object stores can add TTL caches outside core.

## APIs

* `fn make_vfs_inode(mount: MountId, backend_ino: BackendInodeId) -> VfsInodeId`
* `fn split(v: VfsInodeId) -> (MountId, BackendInodeId)`
* `InodeCache::get_or_insert(vfs_inode, factory) -> Arc<NodeRef>`

  * factory calls into mount.fs to obtain a node by id if supported, or is constructed during traversal.

## Tests

* `VfsInodeId` uniqueness across mounts.
* Cache returns same Arc for same inode when still alive.
* Weak eviction works (drop all strong refs then get again yields new Arc).

---

# `vfs/core/src/node.rs` — FsNode abstraction (full)

## Responsibilities

Define the **backend-facing object model** used by core semantics:

* Nodes represent filesystem objects inside a specific mounted FS.
* Nodes must support directory lookup, creation, deletion, rename, link/symlink, open, metadata.
* Node APIs must be expressible for both sync and async (traits or dual traits; actual async traits are in Phase 4, but node.rs must be compatible).

The key requirement: **backends must not require global absolute paths** for correctness. All ops are relative to node handles.

## Object model

### `Fs` (filesystem instance / superblock)

* Returns a root node.
* Provides capability info (via provider registry layer).
* May provide `node_by_inode` if backend supports stable handles.

Minimum:

* `fn root(&self) -> Arc<dyn FsNode>`
* `fn capabilities(&self) -> FsCapabilities`
* Optional: `fn node_by_inode(&self, ino: BackendInodeId) -> Option<Arc<dyn FsNode>>`

### `FsNode`

Nodes are typed by file type but exposed as one trait with checks.

Required methods:

* Identity:

  * `fn inode(&self) -> BackendInodeId`
  * `fn file_type(&self) -> VfsFileType`
* Metadata:

  * `fn metadata(&self) -> VfsResult<VfsMetadata>`
  * `fn set_metadata(&self, set: SetMetadata) -> VfsResult<()>` (chmod/chown/utimens/truncate)
* Directory ops (valid if directory):

  * `fn lookup(&self, name: &VfsName) -> VfsResult<Arc<dyn FsNode>>`
  * `fn create_file(&self, name, opts: CreateFile) -> VfsResult<Arc<dyn FsNode>>`
  * `fn mkdir(&self, name, opts: MkdirOptions) -> VfsResult<Arc<dyn FsNode>>`
  * `fn unlink(&self, name, opts: UnlinkOptions) -> VfsResult<()>`
  * `fn rmdir(&self, name) -> VfsResult<()>`
  * `fn read_dir(&self, cursor: Option<DirCursor>, max: usize) -> VfsResult<ReadDirBatch>`
* Link/rename:

  * `fn rename(&self, old_name, new_parent: &dyn FsNode, new_name, opts: RenameOptions) -> VfsResult<()>`
  * `fn link(&self, existing: &dyn FsNode, new_name) -> VfsResult<()>` (hardlink; capability-gated)
  * `fn symlink(&self, new_name, target: &VfsPath) -> VfsResult<()>`
  * `fn readlink(&self) -> VfsResult<VfsPathBuf>` (valid if symlink)
* Open:

  * `fn open(&self, opts: OpenOptions) -> VfsResult<Arc<dyn FsHandle>>`

**Note on rename signature**

* Rename is best expressed as: `old_parent.rename(old_name, new_parent, new_name, flags)`
* This matches Linux VFS and avoids requiring “path strings”.

### Directory iteration model

Avoid returning a `Vec<DirEntry>` for whole directories. Provide batching:

* `DirCursor` is an opaque cursor (u64) or backend token.
* `ReadDirBatch` includes:

  * `entries: SmallVec<[VfsDirEntry; N]>`
  * `next: Option<DirCursor>`

Core will later implement getdents encoding; node.rs just provides structured entries.

## Error rules

* If a method is called on wrong type (e.g. lookup on file), return `NotDir` / `InvalidInput`.
* If backend lacks feature (hardlinks, symlinks), return `NotSupported` **and** capability flags must reflect that.

## Overlay interaction requirements

`FsNode` must be composable:

* overlay will implement its own `Fs`/`FsNode` that delegates to upper/lower nodes.
* Therefore:

  * keep `FsNode` methods minimal and semantic.
  * avoid leaking backend-specific handles or paths.
  * ensure `inode()` stability rules still hold for overlay nodes (overlay can synthesize inode ids).

## Tests (for memfs first, then hostfs)

* lookup/create/unlink/rename correctness.
* readdir merging order constraints are overlay’s responsibility, but base readdir must be deterministic.

---

If you want the next step, I can do the same “full spec” treatment for `vfs/core/src/vfs.rs` (the top-level dispatcher), including **exact call flows** for openat/statat/renameat with PathWalker + mount + node + handle interactions (and where rights/policy checks must happen), because that’s where these modules snap together.
