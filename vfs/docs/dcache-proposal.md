# Dcache (Directory Lookup Cache) – Design Plan

This document plans how to add a **dcache** (directory/name lookup cache) to the current VFS design and implementation. The dcache caches the result of “lookup parent directory + name → child node” so repeated resolution of the same path (or path prefix) avoids redundant backend lookups.

---

## 1. Goal and scope

- **Goal:** Reduce backend `lookup(parent, name)` calls during path resolution by caching (parent inode, name) → child node at the VFS layer.
- **Scope:** VFS-level cache owned by the same entity that owns the mount table (the `Vfs` facade). Optional feature: when disabled (default or config), path resolution behaves as today (no cache).
- **Non-goals for v1:** Per-mount dcache, distributed/shared dcache, caching readdir results (separate concern).
- **Capability awareness:** The dcache must be aware of the underlying FS capabilities and mount flags: read-only mounts never need invalidation; some FS cannot guarantee consistency with VFS invalidation and must not be cached (see §2.4).

---

## 2. Cache semantics

### 2.1 Key

- **Key:** `(MountId, BackendInodeId, VfsNameBuf)`  
  - Directory is identified by `(MountId, BackendInodeId)` (parent’s backend inode within that mount).  
  - Name is the single path component used in `lookup(parent, name)`.

- **Rationale:**  
  - MountId scopes the cache to one filesystem instance.  
  - Parent’s `BackendInodeId` is stable for the lifetime of the mount (per existing inode contract).  
  - `VfsNameBuf` is already validated and has `Hash + Eq`; no need to store raw bytes.

### 2.2 Value

- **Value:** `DcacheEntry` holding the result of the lookup:
  - **Positive hit:** `Ok(Arc<dyn FsNode>)` (or equivalent `NodeRef`-like handle).  
    On hit, PathWalker constructs `NodeRef::new(mount, node)` and does **not** call `parent.node().lookup(name)`.
  - **Negative hit (optional):** `Err(NotFound)` cached for a bounded time so repeated lookups of missing files don’t hammer the backend.  
    Requires explicit invalidation when a new entry is created under that parent with the same name.

- **Rationale:** Caching the resolved node (not just `VfsInodeId`) avoids a second backend call; backends do not currently expose “get node by inode” and resolution is done via lookup only.

### 2.3 Where the cache is used

- **Read path:** Inside `PathWalker::resolve_internal` (sync) and the async equivalent, when processing `WorkComponent::Normal(name)`:
  1. Compute parent’s `(MountId, BackendInodeId)` from `current`.
  2. Build key `(mount, parent_backend_inode, name)` (using an owned `VfsNameBuf` from the borrowed `VfsName`).
  3. If dcache is enabled: look up key.
     - **Hit (positive):** Use cached node to build `NodeRef`; skip `current.node().lookup(&name_ref)?`.
     - **Hit (negative):** Return `NotFound` (or equivalent) and do not call backend.
     - **Miss:** Call `current.node().lookup(&name_ref)?`; if dcache is enabled, insert positive or negative entry.

- **Write path:** All mutating operations that change directory contents must invalidate the dcache for **writable** mounts (see §2.4 and §4).

### 2.4 Capability and mount awareness

The dcache must respect the capabilities of the underlying FS and the mount flags:

- **Read-only mounts**  
  A mount with `MountFlags::READ_ONLY` is never mutated via the VFS. For such mounts:
  - **Lookups:** Use the dcache as usual (get/insert). Caching is always valid.
  - **Invalidation:** Never invalidate. No VFS operation can change the namespace under a read-only mount, so invalidation is unnecessary and can be skipped for that mount.

- **Cacheable vs non-cacheable mounts**  
  Some backends cannot guarantee that their namespace only changes through VFS operations (e.g. network FS, object store, or FS that does not support reliable invalidation). For those, the dcache must **not** be used, or cached data could become stale.
  - Introduce a provider capability, e.g. **`FsProviderCapabilities::DCACHE_SAFE`** (or the inverse, e.g. `NO_DCACHE` / `EXTERNAL_MUTATIONS`). Semantics:
    - **DCACHE_SAFE set:** The backend’s directory tree only changes via VFS operations that trigger invalidation. Lookups for this mount may be cached and invalidated as in §4.
    - **DCACHE_SAFE unset (or NO_DCACHE set):** The backend may change externally or cannot guarantee consistency with VFS invalidation. Do **not** use the dcache for this mount: no cache lookup, no cache insert. Every resolution goes to the backend.
  - **Per-mount rule:** When resolving a component, the walker has `current.mount()`. Before using the dcache (get or insert), check whether that mount is cacheable:
    - If the mount is **read-only** → cacheable (and never invalidate).
    - Else if the provider has **DCACHE_SAFE** → cacheable (invalidate on mutations).
    - Else → **not** cacheable; skip dcache for this lookup (call backend directly, do not insert).
  - Mount entry can derive this from `MountEntry.flags` (READ_ONLY) and `MountEntry.fs_sync.provider_capabilities()` (or equivalent). If the mount table does not store provider capabilities, the walker can call `current.node()`’s FS capabilities via the snapshot (each MountEntry holds the Fs, which exposes `provider_capabilities()`).

- **Summary**

| Mount state | Use dcache? | Invalidate on mutation? |
|-------------|-------------|--------------------------|
| Read-only (`MountFlags::READ_ONLY`) | Yes | No (no mutations) |
| Writable + `DCACHE_SAFE` | Yes | Yes |
| Writable + not `DCACHE_SAFE` | No | N/A |

---

## 3. Ownership and lifecycle

- **Owner:** The dcache is owned by the same place that owns the mount table. In the current layout that is `VfsInner` (in `vfs/core/src/vfs.rs`). So:
  - `struct VfsInner { mount_table: Arc<MountTable>, dcache: Option<Arc<Dcache>>, ... }`.
- **PathWalker:** Currently `PathWalker::new(mount_table)` only holds `Arc<MountTable>`. For dcache-aware resolution, PathWalker must have access to the dcache:
  - Either add `dcache: Option<Arc<Dcache>>` to `PathWalker` and set it in `Vfs::path_walker()` / `path_walker_async()`, or
  - Pass a `&Dcache` (or `Option<&Dcache>`) into `resolve_internal` via a context that the Vfs creates when calling the walker.

- **Recommendation:** Add `dcache: Option<Arc<Dcache>>` to `PathWalker` (and `PathWalkerAsync`) and pass it from `Vfs`. When `None`, resolve behaves as today (no cache). This keeps the walker API unchanged and keeps all dcache logic behind the optional reference.

---

## 4. Invalidation (when the cache must be updated)

Any operation that changes the namespace under a directory must invalidate the dcache so that subsequent lookups see the new state—**only for mounts that are writable and cacheable** (see §2.4). For read-only mounts, skip invalidation. For mounts that are not dcache-safe, no entries exist. Invalidation should be **by (MountId, parent BackendInodeId)** or by the specific **(MountId, parent, name)** where applicable.

| Operation | Invalidation |
|-----------|--------------|
| **create_file(parent, name)** | Invalidate `(parent.mount, parent.backend_inode, name)` (remove negative entry if present). Optionally invalidate parent’s “directory listing” if a separate list cache exists (out of scope here). |
| **mkdir(parent, name)** | Same as create_file. |
| **symlink(parent, name, _)** | Same. |
| **link(parent, name, _)** | Same. |
| **unlink(parent, name)** | Invalidate `(parent.mount, parent.backend_inode, name)`. |
| **rmdir(parent, name)** | Same. |
| **rename(old_parent, old_name, new_parent, new_name)** | Invalidate `(old_parent.mount, old_parent.backend_inode, old_name)` and `(new_parent.mount, new_parent.backend_inode, new_name)`. If old_parent == new_parent, one invalidation covers both. |
| **mount(provider, target_path)** | The target path resolves to a directory inode; that inode becomes a mountpoint. Invalidate entries for that directory inode in the **parent** mount (the mount that contains the mountpoint): i.e. invalidate by parent directory of the mountpoint. Also, the directory inode’s identity (same backend inode) now refers to the new root of the mounted fs, so any cache entry keyed by that inode as *parent* should be cleared for the mount that contains the mountpoint (entries like `(parent_mount, mountpoint_inode, name)` no longer mean “child of mountpoint dir” but “child of mounted root”). Simplest safe rule: on mount, invalidate all entries for `MountId` of the mount that contains the mountpoint (or invalidate by mountpoint’s parent inode and optionally the mountpoint inode itself). |
| **unmount(target_path)** | Same as mount: the inode under which the mount was attached again becomes the underlying directory. Invalidate the same scope as for mount. |

- **Implementation note:** Before invalidating, resolve the mount and check: if the mount is read-only, **skip invalidation** for that mount. A practical approach is to support two invalidation primitives: (1) **invalidate_one(mount, parent_backend_inode, name)** and (2) **invalidate_children_of(mount, parent_backend_inode)** (clear all entries with that parent). The caller (Vfs mutation path or mount/unmount) must only call these for mounts that are writable and dcache-safe. Then:
  - create_file / mkdir / symlink / link: if parent’s mount is writable and dcache-safe, invalidate_one(parent, name).
  - unlink / rmdir: same.
  - rename: invalidate_one(old_parent, old_name) and invalidate_one(new_parent, new_name) for writable, dcache-safe mounts only.
  - mount / unmount: invalidate_children_of(parent_mount, parent_backend_inode) for the mountpoint’s parent (and optionally by mountpoint inode); the parent mount is the one whose namespace changes.

---

## 5. Bounds and eviction

- **Size limit:** Cap the number of cached entries (e.g. max 1024 or 4096 entries). When the cache is full, use an **LRU eviction** policy so that repeated lookups for the same paths stay hot.
- **Negative entries:** If negative caching is implemented, either (a) give them a short TTL (e.g. 1–5 seconds) and no explicit invalidation beyond create under same parent+name, or (b) invalidate them explicitly on create_file/mkdir/symlink/link under that (parent, name). Option (b) is semantically cleaner and matches the invalidation table above.
- **No TTL for positive entries:** Rely on explicit invalidation and LRU; no time-based expiry for positive entries keeps semantics predictable and avoids stale positives.

---

## 6. Concurrency and sync/async

- **Sync path:** PathWalker runs in a single thread per resolve. The dcache can be behind a `RwLock<DcacheInner>` or a concurrent map (e.g. `DashMap`) for the key–value store. If the same `Vfs` is used from multiple threads, use a thread-safe structure (e.g. `DashMap` + LRU via a separate structure or a crate that supports concurrent LRU).
- **Async path:** PathWalkerAsync also performs one component at a time. Same dcache can be shared between sync and async walkers; use a concurrent map so that concurrent resolves (sync or async) can read the cache without blocking each other. Invalidation (writes) should be short and local to the invalidation primitives.
- **Mount table snapshot:** The mount table already uses a snapshot (`Arc<MountTableInner>`). The dcache does not need to be tied to the snapshot: cache keys use `(MountId, BackendInodeId, name)`, and MountId is stable. On unmount, we invalidate as above; we do not need to “version” the dcache with the mount table.

---

## 7. Optional / configurable

- **Feature flag or config:** Add a `VfsConfig` (or builder) option, e.g. `dcache_max_entries: Option<usize>`. If `None`, dcache is disabled (PathWalker gets `dcache: None`). If `Some(n)`, allocate a dcache with max `n` entries and wire it into PathWalker and into all invalidation points in Vfs.
- **Zero cost when disabled:** When dcache is `None`, the hot path should not touch the dcache at all (no lock, no map lookup). So the existing behavior remains the default and remains zero-cost.

---

## 8. Implementation steps (phased)

### Phase A – Core dcache type and wiring (no PathWalker use yet)

1. **Add capability and mount awareness**
   - In `vfs/core/src/provider.rs`, add **`FsProviderCapabilities::DCACHE_SAFE`** (or equivalent) to the provider capability flags. Semantics: set for backends whose directory tree only changes via VFS operations (e.g. memfs, hostfs); leave unset for backends that may change externally or cannot guarantee invalidation (e.g. network FS, object store).
   - Ensure `MountEntry` (or the path to it) exposes: (a) `MountFlags` (for `READ_ONLY`), (b) provider capabilities (e.g. via `fs_sync.provider_capabilities()` or by storing them on the entry). PathWalker and Vfs need both to implement §2.4.

2. **Add `vfs/core/src/dcache.rs`**
   - Define `DcacheKey { mount: MountId, parent: BackendInodeId, name: VfsNameBuf }` (with `Hash + Eq`).
   - Define `DcacheEntry` (e.g. `Positive(Arc<dyn FsNode>)` and optionally `Negative`).
   - Define `Dcache` struct with:
     - Inner storage: e.g. `DashMap<DcacheKey, DcacheEntry>` or `RwLock<HashMap<DcacheKey, DcacheEntry>>` plus an LRU structure (e.g. `lru::LruCache` or a custom list + map).
     - `get(&self, key: &DcacheKey) -> Option<DcacheEntry>`.
     - `insert(&self, key: DcacheKey, entry: DcacheEntry)` (and evict if over capacity).
     - `invalidate_one(&self, mount: MountId, parent: BackendInodeId, name: &VfsName)`.
     - `invalidate_children_of(&self, mount: MountId, parent: BackendInodeId)` (scan keys and remove matching (mount, parent, _)).
   - Implement LRU eviction when inserting and cache is full (evict oldest used entry).

3. **Integrate into Vfs**
   - Add `dcache: Option<Arc<Dcache>>` to `VfsInner`; construct it when `Vfs` is built with dcache enabled (e.g. from a new `VfsBuilder` or an optional parameter in `Vfs::new`).
   - Pass `self.inner.dcache.clone()` into `PathWalker::new` (and async) so that PathWalker holds `Option<Arc<Dcache>>`. For now, PathWalker still does not use it (next phase).

### Phase B – Use dcache in path resolution

4. **PathWalker sync**
   - In `resolve_internal`, when handling `WorkComponent::Normal(name)` and about to call `current.node().lookup(&name_ref)?`:
     - If `self.dcache.is_some()`, resolve whether **this mount is cacheable** (see §2.4): from the mount table snapshot, get the `MountEntry` for `current.mount()`; if the mount is read-only or the provider has `DCACHE_SAFE`, use the dcache. Otherwise skip dcache for this lookup (call backend directly, do not insert).
     - If cacheable: build `DcacheKey { mount: current.mount(), parent: current.node().inode(), name: name_buf }`. Call `dcache.get(&key)`. If `Some(Positive(node))`, use it to build `NodeRef` and skip backend lookup. If `Some(Negative)`, return `Err(NotFound)`. If `None`, call `current.node().lookup(&name_ref)?`, then insert positive or negative entry.
   - Ensure symlink and mountpoint handling still use the resolved node (same as today).

5. **PathWalker async**
   - Same logic in the async `resolve_internal` path: check cacheability for current mount, then consult dcache on miss call `current.node().lookup(&name_ref).await?`, then fill dcache when cacheable.

### Phase C – Invalidation at mutation sites

6. **Vfs mutation methods**
   - Before calling any invalidation, check that the **parent’s mount is writable and dcache-safe** (see §2.4). If the mount is read-only, **skip invalidation**. If the mount is not dcache-safe, no dcache entries exist for it, so nothing to invalidate.
   - In `openat` (create path), after `create_file`, if parent mount is writable and dcache-safe, call `dcache.invalidate_one(parent.dir.mount, parent.dir.node.inode(), &parent.name)`.
   - In `mkdirat`, after successful `mkdir`, same check then `invalidate_one(parent.dir.mount, parent.dir.node.inode(), &name)`.
   - In `unlinkat`, after successful `unlink`, same check then `invalidate_one(parent.dir.mount, parent.dir.node.inode(), &name)`.
   - In `renameat`, after successful `rename`, call `invalidate_one` for old parent+old name and new parent+new name (each only if that mount is writable and dcache-safe).
   - In `symlinkat`, after successful `symlink`, same check then `invalidate_one(parent.dir.mount, parent.dir.node.inode(), &name)`.
   - Mirror all of the above for the async variants (`openat_async`, `mkdirat_async`, etc.).

7. **Mount / unmount**
   - In `MountTable::mount` (or wherever mount is performed), after attaching the new mount, call `dcache.invalidate_children_of(parent_mount, parent_backend_inode)` for the **parent** mount (the one that contains the mountpoint). The parent mount’s namespace changed; only invalidate if that parent is dcache-safe (read-only or DCACHE_SAFE). The Vfs (or whoever owns the dcache) must have a way to get a reference to the dcache when mounting; e.g. pass `Option<Arc<Dcache>>` into the mount logic or call an invalidation callback provided by Vfs.
   - Same for unmount: after detach, invalidate the same scope (parent mount, if dcache-safe).

### Phase D – Tests and tuning

8. **Tests**
   - Unit tests: dcache get/insert/invalidate_one/invalidate_children_of; LRU eviction when at capacity.
   - Integration: resolve a path twice, second time should hit dcache (mock or memfs backend that counts lookups); then create_file under that path and resolve again, should see new node (invalidated).
   - Mount/unmount: resolve through a mountpoint, then unmount, then resolve again; cache should not return the old mounted root.
   - Capability awareness: resolve on a mount without DCACHE_SAFE (and not read-only) and confirm no dcache get/insert; resolve on a read-only mount and confirm no invalidation is called on mutations elsewhere.

9. **Documentation and tuning**
   - Document dcache in `vfs/core/README.md` and in `fs-refactor.md` (e.g. Phase 8.1 “Caching layer”) as the dcache implementation.
   - Tune default `dcache_max_entries` and consider making negative caching configurable (on/off, and if on, whether to use TTL or only explicit invalidation).

---

## 9. Summary

| Aspect | Choice |
|--------|--------|
| **Key** | `(MountId, BackendInodeId, VfsNameBuf)` |
| **Value** | Positive: `Arc<dyn FsNode>`; optional negative entry |
| **Owner** | `VfsInner` (same as mount table) |
| **Used in** | `PathWalker::resolve_internal` (sync and async) |
| **Invalidation** | On create_file, mkdir, symlink, link, unlink, rmdir, rename; and on mount/unmount (invalidate affected parent/children). **Only for writable, dcache-safe mounts**; read-only mounts never need invalidation. |
| **Capability awareness** | Use dcache only for mounts that are read-only or have `DCACHE_SAFE`. Never cache mounts that cannot guarantee consistency with VFS invalidation. Skip invalidation for read-only mounts. |
| **Bounds** | LRU, max entries configurable; optional negative cache with explicit invalidation |
| **Concurrency** | Concurrent map (e.g. DashMap) if Vfs is shared across threads; otherwise RwLock + LRU is enough |
| **Default** | Disabled (dcache = None) for zero cost and backward compatibility |

This plan keeps the existing path resolution and mount semantics unchanged when dcache is off, and adds a predictable, invalidate-based dcache when enabled, aligned with the existing fs-refactor Phase 8.1 “Caching layer” and Phase 8.4 “Performance-first data structures” goals.
