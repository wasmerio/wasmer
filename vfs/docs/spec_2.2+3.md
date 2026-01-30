
Below is a **detailed, junior-implementable spec** for **Phase 2, Step 2.2 (inode/node identifiers)** and **Step 2.3 (filesystem node interfaces)**, written to fit the plan’s layering contract and the later mount/overlay/handle/WASIX integration steps.

NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan.

---

## Step 2.2 — `vfs/core/src/inode.rs`: Inode + node identifier model

### Goals

1. Provide **small, copyable, hashable IDs** used everywhere (path walker, mount table, caches, watchers, resource table).
2. Avoid collisions across mounts by making inode identity **per-mount namespaced**.
3. Define a **stability contract** that backends can realistically satisfy (hostfs, memfs) and emulate (object store).
4. Keep hot-path operations fast: IDs are POD, no heap allocations.

### Non-goals

* Persistence across process restarts.
* Global uniqueness across separate VFS contexts (each Wasix environment can have its own mount table).
* Perfect parity with Linux inode generation numbers (optional later).

---

### Required types

#### `MountId`

* Type: `NonZeroU32` (use `core::num::NonZeroU32`).
* Rationale:

  * Compact.
  * `Option<MountId>` is niche-optimized (same size).
  * `0` reserved for “invalid/unset”.
* Creation:

  * Only the mount table allocates `MountId`.
  * Provide `MountId::new(u32) -> Option<MountId>` (or keep constructor private and expose via mount table).

#### `BackendInodeId`

* Type: `NonZeroU64`.
* Rationale:

  * Compact, hashable, `Option<BackendInodeId>` is niche-optimized.
  * Works well for memfs (slab index + 1) and synthetic IDs.
* Backend contract:

  * Must be **stable for the lifetime of the mount**.
  * Must **not be reused** within the mount lifetime (prevents stale handle/node confusion).
  * If a backend naturally produces `0`, it **must remap** (e.g., `raw + 1`).

#### `VfsInodeId`

* Type: a plain struct:

  ```rust
  #[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
  pub struct VfsInodeId {
      pub mount: MountId,
      pub ino: BackendInodeId,
  }
  ```
* Must be `Copy` and `Hash` (used as mountpoint keys and caches).

#### `VfsNodeId` (optional alias)

* In many VFS designs, “node id == inode id”. Keep it simple:

  * `pub type VfsNodeId = VfsInodeId;`
* If later you add generation numbers, `VfsNodeId` can become `(inode, gen)` without breaking all call sites.

---

### Stability contract (MUST document in `inode.rs` docs)

**Within a single mounted filesystem instance**:

* If the same filesystem object is looked up multiple times, the backend should return the same `BackendInodeId`.
* IDs remain stable across:

  * repeated lookups,
  * reopening,
  * reading directories,
  * renames (the inode stays the same; directory entries move).
* IDs do **not** need to be stable across unmount/mount or process restarts.

**If a backend cannot provide stable inode identity** (object stores, some virtual providers):

* It MUST implement a **synthetic inode mapping** layer that:

  * assigns a fresh monotonically increasing `BackendInodeId` for each unique object key (and “directory key” for pseudo-dirs),
  * stores a mapping `Key -> BackendInodeId`,
  * never reuses IDs within mount lifetime,
  * supports explicit invalidation (optional) if listing reveals objects are gone.

---

### Synthetic inode mapping (required design for non-POSIX backends)

Add a helper type in `inode.rs` (or `inode::map` submodule) usable by object-store backends later:

#### `InodeKey`

* Represent backend object identity:

  * For object stores: `(bucket, key, version/etag?)`.
  * For pseudo-dirs: a canonical prefix key ending with `/`.
* Type recommendation:

  * `Box<[u8]>` (cheap to clone pointer, exact bytes, no UTF-8 requirement).
  * The backend decides canonical encoding (document it).

#### `SyntheticInodeMap`

* Data:

  * `next: AtomicU64` starting at `1`.
  * `by_key: RwLock<HashMap<InodeKey, BackendInodeId>>`
  * Optionally `by_id: RwLock<HashMap<BackendInodeId, InodeKey>>` (debug / reverse lookups; not required in v1).
* API:

  * `fn get_or_alloc(&self, key: &InodeKey) -> BackendInodeId`
  * `fn invalidate(&self, key: &InodeKey)` (optional)
  * `fn invalidate_prefix(&self, prefix: &[u8])` (optional, for pseudo-dirs)
* Rules:

  * IDs monotonically increase; never reuse.
  * Locking: read-heavy; `RwLock` is fine. (Later can optimize with dashmap.)

---

### Backend guidance (how each backend satisfies 2.2)

#### `vfs-mem`

* Use a slab/slotmap index:

  * `BackendInodeId = NonZeroU64::new((index as u64) + 1).unwrap()`
* Never reuse indices during mount lifetime (if you have deletions, you may keep tombstones or keep free list but DO NOT reuse IDs in v1).

#### `vfs-host`

* Unix:

  * Prefer `(st_dev, st_ino)` to build stable identity.
  * But `BackendInodeId` is a single `u64`: combine dev+ino with a stable hashing scheme or a bijective packing if possible.
  * **Important:** if you hash, collisions are theoretically possible. Prefer packing:

    * If `st_dev` and `st_ino` fit common sizes, pack; otherwise use a per-mount map `(dev, ino) -> BackendInodeId` (like synthetic map but keyed by tuple).
* Windows:

  * Use file ID from `GetFileInformationByHandleEx(FileIdInfo)` or equivalent.
  * If not available, fall back to synthetic map keyed by normalized handle identity (last resort; document limitations).

#### `vfs-overlay`

* Overlay must present its own stable inode IDs within overlay mount.
* Recommended: use a synthetic map keyed by:

  * `(layer_kind, underlying_mount_id, underlying_backend_inode_id)` for passthrough nodes
  * plus overlay metadata for whiteouts/opaque dirs.
* Overlay *must* ensure: “the same merged-visible object” maps to the same `BackendInodeId` while it exists.

---

### Integration points (why 2.2 matters elsewhere)

* **PathWalker (2.1)**:

  * When traversing components, each `lookup()` must yield a node with a stable `BackendInodeId`.
* **Mount table (3.1)**:

  * Mount transitions are keyed by `VfsInodeId` of the mountpoint in the parent mount.
* **Watchers (optional subsystem)**:

  * Watches attach to `VfsInodeId`, not paths, to survive renames.
* **Wasix FD table (Phase 5)**:

  * Directories and file handles should store `VfsInodeId` for identity / debugging / watch registration.

---

### Step 2.2 Acceptance criteria

* Unit tests in `vfs-core`:

  1. `VfsInodeId` is `Copy`, `Eq`, `Hash`.
  2. Two mounts can use the same `BackendInodeId` without colliding in hash maps (`VfsInodeId` differs by mount).
  3. `SyntheticInodeMap` returns stable IDs for the same key, distinct IDs for different keys, and never returns `0`.

---

---

## Step 2.3 — `vfs/core/src/node.rs`: Filesystem node interfaces

### Goals

1. Define the **primitive backend-facing node operations** used by `PathWalker`, mounts, overlay, and later Wasix syscalls.
2. Ensure the interface supports **at-style semantics** (parent directory handle + name), and does not encourage absolute-path backends.
3. Support both **sync and async** usage paths without duplicating semantics in Wasix.
4. Keep it implementable for memfs immediately; hostfs and overlay later.

### Non-goals

* Full FD/OFD semantics (that’s Step 2.4).
* Full mount traversal logic (PathWalker + mount table handle that).
* Watch API (optional subsystem later).

---

### Key design constraints (from the plan’s layering contract)

* Backends must not require global absolute paths for correctness.
* `openat` / `renameat` / `unlinkat` must be doable via:

  * “directory node (base)” + “name” + flags
* `PathWalker` is the only component performing path resolution and symlink chasing.

---

### Core concepts and types in `node.rs`

#### `VfsName` (component name type)

Because Wasix/WASI paths are byte sequences and must support non-UTF8:

* Use a borrowed slice for hot paths:

  * `pub type VfsName<'a> = &'a [u8];`
* Rules:

  * `VfsName` must not contain `/` and must not be empty.
  * PathWalker enforces these rules before calling backend ops.
* For stored names (dir entries), backends can return `Box<[u8]>` or `Vec<u8>`.

#### `VfsNodeKind`

Minimal node type classification:

```rust
pub enum VfsNodeKind {
  File,
  Directory,
  Symlink,
  Special, // fifo, char, block, socket (hostfs later)
}
```

#### `VfsLookupFlags`

Used by PathWalker, not by backends for policy:

* `FOLLOW_SYMLINKS_INTERMEDIATE` (PathWalker controls chasing)
* `NO_FOLLOW_LAST` (for `O_NOFOLLOW` / `AT_SYMLINK_NOFOLLOW`)
  In v1, backends don’t implement symlink policy; they only expose `readlink()` and return metadata kind.

#### `VfsDirCookie`

For incremental directory reading (getdents-friendly):

* `pub struct VfsDirCookie(pub u64);`
* `0` means “start”.
* Backends may ignore cookies initially and return “all entries” for memfs; hostfs should support cookies if feasible.

#### `VfsDirEntry`

Returned from directory enumeration:

```rust
pub struct VfsDirEntry {
  pub name: Box<[u8]>,
  pub ino: BackendInodeId,     // backend-local inode
  pub kind: VfsNodeKind,       // if known cheaply, else Special/Unknown
}
```

---

### Backend-facing node traits

> **Important:** Step 4 will split these into `traits_sync.rs` and `traits_async.rs`.
> For Step 2.3, implement them in `node.rs` (or define them here and re-export later). The key is: **memfs must implement them now**, and `PathWalker` must call them.

#### `trait FsNodeSync`

This is the primitive “node handle” inside a mounted filesystem.

**Required bounds**

* `Send + Sync + 'static` (so registry can store `Arc<dyn ...>` safely).

**Identity**

* `fn inode(&self) -> BackendInodeId;`
* `fn kind(&self) -> VfsNodeKind;` (or `metadata().kind`, but kind is hot in PathWalker)

**Lookup (directory only)**

* `fn lookup_child(&self, name: VfsName<'_>) -> VfsResult<Arc<dyn FsNodeSync>>;`

  * Errors:

    * If `self` is not a directory: `NotDir`
    * If child not found: `NotFound`

**Create / remove / link operations (directory only)**

* `fn create_file(&self, name: VfsName<'_>, opts: CreateFileOpts) -> VfsResult<Arc<dyn FsNodeSync>>;`
* `fn create_dir(&self, name: VfsName<'_>, opts: CreateDirOpts) -> VfsResult<Arc<dyn FsNodeSync>>;`
* `fn remove_file(&self, name: VfsName<'_>) -> VfsResult<()>;`
* `fn remove_dir(&self, name: VfsName<'_>) -> VfsResult<()>;`
* `fn link(&self, new_name: VfsName<'_>, existing: &dyn FsNodeSync) -> VfsResult<()>;`

  * Only if hardlinks supported (capability gating later).
* `fn symlink(&self, name: VfsName<'_>, target: &[u8]) -> VfsResult<Arc<dyn FsNodeSync>>;`

**Rename**

* Rename must be parent-directory based:

  * `fn rename_child(&self, old_name: VfsName<'_>, new_parent: &dyn FsNodeSync, new_name: VfsName<'_>) -> VfsResult<()>;`
* VFS core enforces:

  * same mount check (or mount table returns `EXDEV` before calling backend)
  * policy checks
* Backend enforces:

  * atomicity if it claims it (capabilities later)
  * correct errors for missing entries, non-empty dirs, etc.

**Symlink read**

* `fn readlink(&self) -> VfsResult<Box<[u8]>>;`

  * If not symlink: `InvalidInput` (or `NotSupported`, but `EINVAL` mapping is typical).

**Metadata**

* `fn metadata(&self) -> VfsResult<VfsMetadata>;`
* `fn set_metadata(&self, set: SetMetadata) -> VfsResult<()>;`

  * `SetMetadata` includes chmod/chown/utimens/truncate-size later (2.5). For now it can be a struct with optional fields; memfs implements subset.

**Open**

* `fn open(&self, opts: OpenOpts) -> VfsResult<Arc<dyn FsHandleSync>>;`

  * `FsHandleSync` is introduced in Step 2.4, but node.rs must define the contract now:

    * backends return a backend handle object; VFS wraps into OFD in 2.4.

**Read directory**

* Prefer a batched API for performance:

  * `fn read_dir(&self, cookie: VfsDirCookie, max: usize) -> VfsResult<(Vec<VfsDirEntry>, Option<VfsDirCookie>)>;`
  * `cookie == 0` starts enumeration.
  * Return `None` when finished.

> Memfs can implement this trivially using a stable ordered map.

---

#### `trait FsNodeAsync`

Same operations, but async-trait based, object-safe.

* Use `async-trait` (per plan).
* Mirror the signatures:

  * `async fn lookup_child(...) -> VfsResult<Arc<dyn FsNodeAsync>>;`
  * etc.

**Adapters**

* Do not implement adapters here; Step 4 handles `AsyncAdapter<T: FsNodeSync>` and `SyncAdapter<T: FsNodeAsync>` via `vfs/rt` hooks.
* But node.rs must keep the traits “clean” enough that those adapters are feasible.

---

### VFS-level node wrapper (strongly recommended)

Backends return `BackendInodeId`, but VFS logic needs `VfsInodeId` (mount + ino).

Define:

```rust
pub struct VfsNode {
  pub id: VfsInodeId,
  pub inner: Arc<dyn FsNodeSync>, // or FsNodeAsync depending on selected surface
}
```

**Why this matters**

* Mount table keys are `VfsInodeId` (3.1).
* Watch registrations attach to `VfsInodeId`.
* Debugging and correctness checks (cross-mount rename) become trivial.

**Construction**

* Only the mounted filesystem wrapper constructs `VfsNode`:

  * `VfsNode { id: VfsInodeId { mount, ino: inner.inode() }, inner }`

---

### Operation semantics & error rules (must align with later layers)

#### Directory-only operations

Backends must return `NotDir` when called on non-directory nodes:

* lookup_child
* create/remove/rename/link/symlink under parent
* read_dir

#### Trailing slash and symlink-follow semantics

* Backends do **not** interpret trailing slashes or do path parsing.
* PathWalker decides:

  * whether the final node must be a directory (trailing slash),
  * whether to follow final symlink.
* Backends simply expose:

  * node kind,
  * readlink content,
  * lookup children for directories.

#### Cross-mount protections

* `rename_child` and `link` must be called only when VFS has already validated same-mount constraints.
* If a backend gets a `new_parent` from a different mount anyway, it may return `CrossDevice` defensively, but **the VFS should prevent it**.

#### Capability-aware behavior (ties into 1.3 and 3.x)

Backends will later declare capabilities like HARDLINK, SYMLINK, RENAME_ATOMIC.
In node APIs:

* If unsupported, return `NotSupported` (mapped to `ENOTSUP` by `vfs/unix`).
* Do not fake support silently.

---

### Confinement requirements (ties into Phase 6 hostfs)

This node interface intentionally pushes you toward correct confinement:

* `lookup_child(dir, name)` rather than `lookup("/abs/path")`.
* `open(node, opts)` rather than `open("/abs/path")`.
* `rename_child(old_parent, name, new_parent, name)` rather than rename by strings.

Hostfs can implement these using:

* Unix: `openat` / `fstatat` / `unlinkat` / `renameat`
* Windows: handle-based APIs, not string prefixing

---

### Performance requirements

* Node operations are on hot paths; avoid allocations:

  * pass names as `&[u8]`,
  * return `Box<[u8]>` for directory entry names (single allocation per entry).
* Prefer `Arc<dyn Trait>` handles:

  * cheap clones,
  * object-safe registry compatibility.
* Directory read batching is required (`read_dir(cookie, max)`), even if memfs ignores cookie initially.

---

### Tests for Step 2.3 (must add in `vfs-core` and/or `vfs-mem`)

Using memfs as the reference backend:

1. **Lookup semantics**

   * create `/a`, `/a/b`, ensure `lookup_child` works and errors correctly.
2. **Directory-only enforcement**

   * calling `lookup_child` on a file returns `NotDir`.
3. **Rename within same directory**

   * rename `a -> b`, ensure inode id stays the same (if backend supports).
4. **Directory iteration**

   * `read_dir` returns deterministic order (memfs should be deterministic for tests).
   * cookie pagination works (or is a no-op but still returns consistent results).
5. **Symlink readlink**

   * create symlink, ensure `readlink` returns expected bytes.
6. **Hardlink gating**

   * if memfs supports hardlinks: link increases link count (metadata step later), and lookup resolves to same inode.

---

### Step 2.3 Acceptance criteria

* `vfs-core` compiles with the full node trait surface defined.
* `vfs-mem` implements `FsNodeSync` (and optionally `FsNodeAsync` via adapter later).
* PathWalker (2.1) can traverse a memfs tree using:

  * `lookup_child`
  * `kind`
  * `readlink` (for symlink traversal tests)
* Directory enumeration works via `read_dir(cookie, max)` and is used by at least one test.

---

## What to actually implement now (concrete deliverables checklist)

### In `vfs/core/src/inode.rs`

* [ ] `MountId`, `BackendInodeId`, `VfsInodeId` types with derives.
* [ ] Documentation of stability + non-reuse contract.
* [ ] `SyntheticInodeMap` helper with `get_or_alloc`.
* [ ] Unit tests for ID behavior + synthetic map.

### In `vfs/core/src/node.rs`

* [ ] `VfsName`, `VfsNodeKind`, `VfsDirCookie`, `VfsDirEntry`.
* [ ] `FsNodeSync` trait with the full operation set above.
* [ ] `FsNodeAsync` trait (same surface) using `async-trait` (even if no backend implements it yet).
* [ ] `VfsNode` wrapper struct tying `MountId` + `BackendInodeId` into `VfsInodeId`.
* [ ] Tests in memfs verifying lookup/create/remove/rename/read_dir/readlink basics.

---

If you want, I can also include **exact Rust signature blocks** for `OpenOpts/CreateFileOpts/SetMetadata` (minimal v1 versions that won’t paint you into a corner for Steps 2.4 and 2.5), but the above is the core spec for implementing 2.2 + 2.3 cleanly.
