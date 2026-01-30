
Below is a **very detailed implementation spec for Phase 3.1 (Mount table design)**, written to be **actionable for a junior developer**, and explicitly tying 3.1 into the rest of the plan (PathWalker, inode IDs, provider registry, overlay, Wasix integration, FD/OFD semantics, error mapping, perf constraints).

IMPORTANT NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan!!!

I’m going to treat your existing `mount.rs` as the starting point and specify **exactly what to keep**, **what to change**, and **what to add** so that 3.1 is “done” in a way that won’t get ripped up later by 3.3/5.x.

---

# Spec: Phase 3.1 — Mount Table and Mount Semantics (`vfs/core/src/mount.rs`)

## 0) Purpose and Scope

### Goal

Implement a Linux-like mount table in the VFS core that supports:

* **Nested mounts** (mount inside a mounted filesystem)
* **Fast mount transitions during path walking**, using **inode-driven** mountpoint detection (not path-prefix)
* **Correct `..` mount boundary behavior** (from mount root go back to parent mount’s mountpoint)
* **Unmount semantics** matching Linux:

  * default unmount fails with **EBUSY** if open handles exist
  * optional lazy unmount (“detach”) hides mount from namespace but keeps it alive until last reference drops

### Out of Scope (explicitly)

* Overlay semantics (3.4) beyond “mount table can mount an overlay Fs like any other Fs”
* Provider registry APIs (3.2) except “mount table must support being called by registry-layer”
* Mount namespaces (not required now; design should not preclude it)
* Persistent mountpoint paths (we are inode-driven; “target path” is not required in the mount table)

---

## 1) How 3.1 connects to other phases

### 3.1 ↔ Phase 2.1 PathWalker

PathWalker must:

* detect mount transitions by checking whether a looked-up child inode is a mountpoint
* treat `..` specially when current inode is the mount root inode
* hold a stable snapshot of mount table during traversal (RCU-like behavior)

**Therefore 3.1 must provide:**

* `enter_if_mountpoint(inner, current_mount, child_inode) -> Option<MountId>`
* `mount_root(inner, mount) -> Option<(VfsInodeId, Arc<dyn Fs>)>`
* `parent_of_mount_root(inner, mount) -> Option<(MountId, VfsInodeId)>`

You already have these helpers—good. We’ll tighten their invariants and error behavior.

### 3.1 ↔ Phase 2.2 inode IDs

Mount detection is keyed by `VfsInodeId = (MountId, BackendInodeId)`:

* A mountpoint inode **must belong to the parent mount** (you already validate this in `mount()`).
* The “root inode” of a mounted fs is also expressed as a `VfsInodeId` whose mount is the new MountId.

So 3.1 must ensure:

* mountpoint inode uniqueness in `mount_by_mountpoint`
* root inode always has `mount == entry.id`

### 3.1 ↔ Phase 2.4 OFD and open handles

Unmount busy-ness depends on open file descriptions / handles that reference a mount’s Fs instance. The mount table needs a **liveness reference counter**. You already have `MountGuard` + `open_count` in `MountEntry`.

Therefore 3.1 must specify:

* who calls `MountTable::guard(mount)`
* when guards are held (at least during path resolution + handle open)
* lazy unmount semantics based on `open_count`

### 3.1 ↔ Phase 3.2 provider registry

MountTable should be *purely* “attach/detach mounts by inode”, while ProviderRegistry owns “create Fs from provider config” and then calls mount table.

So mount table API should stay:

* `mount(parent_mount, mountpoint_inode, fs, root_inode, flags) -> MountId`
* `unmount(target_mount, flags)`

…and 3.2 will build a higher-level `mount_at_path()` helper elsewhere.

### 3.1 ↔ Phase 5 Wasix integration

Wasix “at”-style syscalls must not resolve mounts itself. It will call `PathWalker`, which uses mount table snapshot. So mount table must:

* be thread-safe
* be low overhead for reads
* support concurrent mount/unmount without corrupting traversal

You already use snapshotting with `Arc<MountTableInner>` under an `RwLock`. That’s acceptable to start (like a simplified ArcSwap/RCU). We’ll formalize correctness constraints.

---

## 2) Terminology and Invariants

### Entities

* **MountEntry**: a mounted filesystem instance in the VFS namespace.
* **MountId**: small integer ID that uniquely identifies a mount within the mount table.
* **mountpoint**: inode in the parent mount where the child mount is attached (stored as `VfsInodeId`).
* **root_inode**: the inode representing the root of the mounted filesystem within VFS (`VfsInodeId` with `mount == MountId`).

### Must-hold invariants (enforced by code)

For any mount entry `m`:

1. If `m.parent.is_none()` then it is the root mount (table root).
2. If `m.parent.is_some()` then `m.mountpoint.is_some()`.
3. If `m.mountpoint = Some(mp)` then `mp.mount == m.parent.unwrap()`.
4. `m.root_inode.mount == m.id`.
5. For any inode `mp` in `mount_by_mountpoint`, it maps to exactly one MountId.
6. `children_by_parent[parent]` contains each child mount at most once.

### “Visibility” invariant for detached mounts

If a mount is lazily unmounted (detached):

* it must **not** be reachable from path traversal (must be removed from `mount_by_mountpoint`)
* it may still be referenced by existing handles/guards
* once `open_count` reaches 0 and it is detached, it can be freed from `mounts[]` slot

This requires some cleanup logic (details below).

---

## 3) Data Structures

### Current structures (keep, but adjust semantics)

You have:

* `MountTable { inner: Arc<RwLock<Arc<MountTableInner>>> }`
* `MountTableInner { root, mounts: Vec<Option<Arc<MountEntry>>>, mount_by_mountpoint: HashMap<VfsInodeId, MountId>, children_by_parent: HashMap<MountId, SmallVec<[MountId;4]>> }`
* `MountEntry { id, parent, mountpoint, root_inode, fs, flags, state, open_count }`

✅ This matches the plan’s intended inode-keyed mount transitions and snapshot publication.

### Required tweaks

#### 3.1.A Add “generation” for debug/consistency (optional but recommended)

Add to `MountTableInner`:

* `pub generation: u64` (monotonic counter incremented on each mount/unmount)
  Reason:
* helps diagnose race reports (“PathWalker used snapshot gen 12 while mount table is gen 14”)
* helps tests assert updates applied

This is optional for functionality, but cheap and very helpful.

#### 3.1.B Add explicit slot freeing

Currently `unmount()` removes indices from maps, but it does not free the `mounts[target_mount]` slot. That means:

* `mounts` keeps the entry forever (memory + it can still be “found”)
* `guard()` can still succeed for unmounted mounts (bad)

We need a clear lifecycle:

* **Active**: reachable and usable
* **Detached**: not reachable; may still be guarded by existing refs
* **Freed**: slot set to `None`; cannot be guarded; ID may be reused later

So, implement a cleanup stage that sets `inner.mounts[target_mount] = None` when safe.

---

## 4) Concurrency Model

### Design

* Readers (PathWalker and most ops) must be cheap:

  * call `snapshot()` (takes read lock, clones Arc)
  * hold `Arc<MountTableInner>` for traversal
* Writers (mount/unmount) are rarer:

  * take write lock
  * clone current inner snapshot
  * mutate clone
  * publish new `Arc<MountTableInner>` into the lock

### Correctness guarantees

* A snapshot is immutable and safe to use without locks.
* A mount/unmount does not affect existing snapshots.
* Lazy unmount must remove reachability in the newly published snapshot immediately.

### Performance note

This is “poor man’s ArcSwap”. It’s acceptable for v1. If later perf requires, we can swap `RwLock<Arc<...>>` → `ArcSwap<...>` with minimal API changes because all callers use `snapshot()`.

---

## 5) Public API (MountTable)

### 5.1 Constructor

#### `MountTable::new(root_fs: Arc<dyn Fs>) -> VfsResult<Self>`

* Creates MountId 0 as root mount
* Sets `root_inode` to `(MountId 0, root_fs.root().inode())`
* Root entry has `parent = None`, `mountpoint = None`, `flags = empty`

✅ Already implemented.

### 5.2 Snapshot

#### `snapshot(&self) -> Arc<MountTableInner>`

* Returns a stable snapshot for PathWalker

✅ Already implemented, but: current error fallback returns an empty inner if lock poisoned. Prefer returning an error instead. Poisoning indicates a bug; silently continuing can corrupt semantics.

**Spec change**:

* Replace fallback with `expect` or return `VfsErrorKind::Internal`.
* This is core infra; “continue with empty mounts” will create nonsense behavior.

### 5.3 Mount creation

#### `mount(&self, parent_mount: MountId, mountpoint_inode: VfsInodeId, fs: Arc<dyn Fs>, root_inode: BackendInodeId, flags: MountFlags) -> VfsResult<MountId>`

**Required behavior**

1. Validate `mountpoint_inode.mount == parent_mount`, else `InvalidInput ("mount.parent_mismatch")` ✅
2. Validate parent mount exists and is active (slot is Some and state Active), else `NotFound ("mount.parent_not_found")`
3. Validate mountpoint is not already used as mountpoint, else `AlreadyExists ("mount.exists")`
4. Allocate a MountId:

   * reuse empty slot if available; else extend vector
5. Create `entry.root_inode = make_vfs_inode(entry_id, root_inode)`
6. Insert in:

   * `mounts[entry_id] = Some(entry)`
   * `mount_by_mountpoint[mountpoint_inode] = entry_id`
   * `children_by_parent[parent_mount].push(entry_id)`
7. Publish updated snapshot

✅ Mostly implemented.

**Spec additions**

* Ensure `children_by_parent` doesn’t accumulate duplicates in weird re-mount sequences:

  * when reusing slot IDs, ensure it’s not still present in parent children list (shouldn’t be, if unmount cleans correctly; but guard anyway in debug assertions).

### 5.4 Unmount

#### `unmount(&self, target_mount: MountId, flags: UnmountFlags) -> VfsResult<()>`

We define Linux-like semantics:

##### Rule 1: Root mount cannot be unmounted

* If `target_mount == inner.root`: return `PermissionDenied` or `InvalidInput` with key `"unmount.root_forbidden"`.

##### Rule 2: Busy handling

* If `open_count > 0`:

  * if `flags == Detach`: mark mount entry state Detached **and proceed with detaching**
  * else: return `Busy ("unmount.busy")` ✅

##### Rule 3: Detach makes mount unreachable immediately

Even if busy, Detach must:

* remove mountpoint mapping: `mount_by_mountpoint.remove(mountpoint)`
* remove from parent’s child list
  This makes future path resolutions stop entering it.

✅ You already do these removals.

##### Rule 4: Slot freeing rules

We need a clear policy:

* If `open_count == 0` at unmount time (non-busy):

  * fully remove entry:

    * remove mountpoint mapping
    * remove from parent children list
    * remove `children_by_parent[target_mount]` entry (if any) **only if we require “unmount requires no children”** (see next rule)
    * set `inner.mounts[target_mount] = None`
* If `open_count > 0` and `Detach`:

  * entry remains stored so guards/handles can keep it alive
  * but once `open_count` drops to 0, we must reclaim slot

##### Rule 5: Unmount of a mount with children

Linux generally requires unmounting children first (unless detach cascade is explicitly implemented). For v1, keep it simple and safe:

* If `children_by_parent[target_mount]` is non-empty (meaning target has active child mounts):

  * return `Busy ("unmount.has_children")`
  * even for Detach, return Busy (unless you explicitly choose “detach cascade”)

**Why**: detach cascade gets complicated quickly (children’s `..` semantics break if parent disappears, unless you detach entire subtree consistently). Avoid for v1.

So implement:

* `if inner.children_by_parent.get(&target_mount).map(|v| !v.is_empty()).unwrap_or(false) { return Err(Busy) }`

##### Rule 6: Lazy cleanup on last close

To reclaim slots after detach, implement one of these approaches:

**Option A (recommended): explicit cleanup call**

* Add `MountTable::try_reclaim(&self, mount_id: MountId)` which:

  * takes write lock, clones inner
  * checks `entry.state == Detached && entry.open_count == 0`
  * if so, sets `inner.mounts[mount_id] = None`, removes `children_by_parent[mount_id]` key, publish
* Call `try_reclaim()` from `MountGuard::drop()` when open_count hits 0

To do that efficiently, `MountGuard` needs a back-reference to MountTable or a cleanup callback. That’s a design decision.

**Option B: no automatic reclamation**

* Accept that detached mounts remain in `mounts[]` forever for v1
* But then `guard()` must refuse detached mounts and `mount()` should reuse None slots only (won’t happen), which causes resource leak.

So **Option A** is the viable one.

**Concrete spec choice: Option A**

* Change `MountGuard` to include:

  * `mount_table: MountTable` (clone is cheap)
* Then in `Drop`:

  * decrement open_count
  * if new_count == 0 and entry.state == Detached, call `mount_table.reclaim_detached(entry.id)` (best-effort; ignore lock poison)

This is important because “detach” is explicitly in plan 3.1 and must actually release resources eventually.

---

## 6) MountGuard Semantics

### Purpose

`MountGuard` prevents unmount from freeing a mount while operations are using it, by incrementing `open_count`.

### Who uses it

* PathWalker should acquire a guard for:

  * the **current mount** while resolving components (at minimum while calling into that mount’s `fs` to lookup children)
* Handle creation should acquire a guard for:

  * the mount that owns the inode being opened
  * and store/clone it into the resulting `VfsHandle` so the mount remains alive as long as the handle exists

This ties 3.1 directly into Phase 2.4 OFD/handles.

### Required behavior changes

* `MountTable::guard(mount)` must fail if:

  * mount slot is None
  * mount entry state is Detached (because detached mounts should not be reachable for new operations)
    Return `NotFound ("mount.guard")` or a dedicated `"mount.detached"` error key.

---

## 7) Path traversal hooks (interface contract for Phase 3.3)

These are “read-only” helpers used by PathWalker; keep them lightweight and pure.

### 7.1 Enter mount

`enter_if_mountpoint(inner, current_mount, child_inode) -> Option<MountId>`

**Spec**

* Ignore `current_mount` (the mapping is keyed by inode which already contains its mount)
* Return mounted MountId only if:

  * mapping exists
  * mapped mount exists and is Active

Implementation detail:

* Since snapshots may contain stale entries, check the slot exists and state active.
* If mapping points to a detached mount (shouldn’t in newest snapshot, but older snapshots might): treat as “no mount”.

### 7.2 Get root of a mount

`mount_root(inner, mount) -> Option<(VfsInodeId, Arc<dyn Fs>)>`

**Spec**

* Return only if mount exists and is Active or Detached?

  * PathWalker will only ask for mounts it intends to enter, so treat Detached as absent.
* However, existing open handles may need fs access even if Detached. That access should come from the handle’s own stored reference, not from `mount_root()`.

So for `mount_root()` used in traversal:

* **only return Active**.

### 7.3 Parent of mount root

`parent_of_mount_root(inner, mount) -> Option<(MountId, VfsInodeId)>`

**Spec**

* Only meaningful if mount has a parent.
* Used by PathWalker when resolving `..` at mount root.
* If mount is Detached, `..` semantics for existing handles are tricky; for v1:

  * PathWalker should never be traversing Detached mounts.
  * Existing open handles that are already “inside” the detached mount are allowed to operate, but path traversal from them should behave consistently. Easiest: handles keep the mount alive and still have parent linkage info in entry, so `..` works as long as you can access it. That suggests `parent_of_mount_root()` should return parent even if Detached **when called from a handle-based walker that already holds a guard**.

To keep the API simple:

* return parent info regardless of state, but only if entry exists.
* the caller decides whether to use it.

---

## 8) Error model and mapping (ties into `vfs/unix`)

3.1 should use these `VfsErrorKind` consistently:

* `InvalidInput`: parent mismatch, unmount root
* `NotFound`: mount not found, parent not found
* `AlreadyExists`: mountpoint already used
* `Busy`: open handles exist; mount has child mounts
* `Internal`: lock poisoned (but avoid silent fallbacks)

Do not map to WASI errno here; that happens in `vfs/unix/src/errno.rs` later.

---

## 9) Testing Requirements (Phase 3 acceptance criteria coverage)

Add tests in `vfs/core/tests/mount_table.rs` (or unit tests in mount module).

### 9.1 Basic mount enter

* Create root fs (memfs)
* Create directory `/mnt`
* Mount another fs at `/mnt`
* Resolve path `/mnt/file` and verify traversal uses mounted fs

This will require PathWalker later; for 3.1 you can test table mechanics directly:

* assert `mount_by_mountpoint` contains mountpoint inode
* assert `enter_if_mountpoint()` returns child mount

### 9.2 `..` across mount boundary

* Mount fs B at inode X in fs A
* Use `parent_of_mount_root()` to verify it returns `(A, X)`
* Later PathWalker test will confirm behavior

### 9.3 Unmount busy fails

* Mount fs at mountpoint
* Acquire `guard(child_mount)`
* Attempt `unmount(child_mount, None)` => `Busy`

### 9.4 Lazy unmount hides mount but keeps existing guard alive

* Mount fs
* Acquire `guard(child_mount)` (open_count > 0)
* `unmount(child_mount, Detach)` succeeds
* Verify `mount_by_mountpoint` no longer contains mountpoint
* Verify `guard(child_mount)` for a *new* guard fails (Detached or NotFound)
* Drop old guard and confirm reclamation frees slot:

  * after dropping last guard, `inner.mounts[child_mount]` becomes None (in a fresh snapshot)

### 9.5 Unmount with children fails

* Mount child at `/mnt`
* Mount grandchild at `/mnt/submnt` (requires mountpoint inode from child)
* Attempt to unmount child => `Busy ("unmount.has_children")`

---

## 10) Implementation Tasks (Junior-friendly checklist)

### Task A — Tighten snapshot behavior

* [ ] Remove `unwrap_or_else` fallback in `snapshot()`
* [ ] Return internal error or panic with clear message (prefer error)

### Task B — Prevent guarding detached mounts

* [ ] Update `MountTable::guard()` to check `entry.state() == Active`
* [ ] Add `VfsErrorKind::NotFound` or `VfsErrorKind::PermissionDenied` for detached (choose one and document)

### Task C — Enforce root unmount forbidden

* [ ] In `unmount()`, if `target_mount == inner.root`, return error

### Task D — Enforce “no children” unmount rule

* [ ] In `unmount()`, fail if `children_by_parent[target_mount]` non-empty

### Task E — Implement detach reclamation

* [ ] Decide mechanism: `MountGuard` needs a way to trigger cleanup
* [ ] Recommended:

  * Add `MountGuard { entry, table: MountTable }`
  * `MountTable::reclaim_detached(mount_id)` does write-lock clone, checks `state == Detached && open_count == 0`, sets slot None, removes `children_by_parent[mount_id]`
* [ ] In `Drop`, after decrement, if new_count == 0 and entry Detached, call reclaim

### Task F — Ensure `unmount()` fully removes non-busy mounts

* [ ] If `open_count == 0` and passes checks:

  * remove mountpoint mapping
  * remove from parent children list
  * set `mounts[target_mount] = None`
  * remove `children_by_parent[target_mount]` entry
* [ ] Publish snapshot

### Task G — Harden traversal helpers against stale/detached mapping

* [ ] In `enter_if_mountpoint()`, verify mapped mount exists and is Active before returning
* [ ] In `mount_root()`, only return Active mounts for traversal usage

### Task H — Tests

* [ ] Implement tests listed above

---

## 11) “Done” definition for Step 3.1

Step 3.1 is complete when:

* Mount transitions can be determined in O(1) by inode mapping.
* `unmount` correctly implements:

  * EBUSY for open mounts (unless detach)
  * detach hides mount from traversal immediately
  * mount cannot be unmounted if it has children
  * root cannot be unmounted
  * detached mounts are reclaimed when last reference drops
* `guard()` cannot be acquired for detached mounts (prevents “new entrants”)
* Tests cover:

  * mount enter mapping
  * unmount busy
  * detach semantics
  * reclaim semantics
  * child mount prevention

---

## 12) Notes on your current code (what’s already good, what must change)

### Already good

* inode-driven mapping (`mount_by_mountpoint: HashMap<VfsInodeId, MountId>`)
* `children_by_parent` structure for hierarchy
* snapshot publishing model (clone inner, publish Arc)
* busy-vs-detach branching
* mountpoint parent mismatch validation
* `MountGuard` open_count mechanics

### Must change for correctness/completeness

* `snapshot()` must not silently return an empty table on lock failure
* `unmount()` currently does **not** free the mount slot (leak + allows guarding unmounted mounts)
* `guard()` currently allows guarding mounts even after detach/unmount (must be blocked)
* must enforce “unmount with children” rule
* must forbid unmounting root
* detach must eventually reclaim resources (otherwise “lazy unmount” is only half implemented)

---

If you want, I can also give you a **patch-style pseudo-diff** showing the exact changes to your `MountGuard` and `MountTable::unmount()` flow (still as a spec, but very close to code), without pulling in PathWalker yet.
