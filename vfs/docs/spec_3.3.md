
IMPORTANT NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan!!!

## Step 3.3 Spec — Path resolution with mounts (integrate into `PathWalker`)

### Goal of step 3.3

Extend the already-built Phase 2 `PathWalker` (normalization, `.`/`..`, symlinks, trailing slash rules, `openat` base semantics) so it becomes **mount-aware** and implements **Linux-like mount traversal**:

* Entering a mount when a looked-up child inode is a mountpoint
* Handling `..` correctly at a mount root (crossing back to the parent mount’s mountpoint)
* Ensuring symlink resolution interacts correctly with mounts
* Ensuring “at-style” calls (base directory + relative path) remain correct when mounts are involved

This step is the *runtime hot path* for most VFS operations, so it must be correct, lock-light, and allocation-minimal.

---

## Dependencies and how they affect 3.3

### Depends on Phase 2 behavior

3.3 must preserve everything from 2.1–2.5:

* **Normalization and traversal rules** from 2.1 stay authoritative (including trailing slash, `NOFOLLOW`, symlink depth, `ENOTDIR`/`ENOENT` distinctions).
* **Inode identity** from 2.2 (`VfsInodeId = (MountId, BackendInodeId)`) is the key for mount transitions. Avoid string prefix mount checks.
* **Node interface** from 2.3 provides `lookup(parent, name)` and metadata queries needed to detect mountpoints and verify `..` behavior.
* **Handle/OFD** from 2.4 matters for base-directory operations (openat-style): base FD refers to a directory node/inode within a mount; traversal must carry mount context forward.
* **Permissions/policy** from 2.5: path walking must offer hooks to enforce jail/chroot-ish “stay in namespace” policy, including on mount boundary traversal (especially for `..` at mount root).

### Depends on Phase 3.1 mount table design

3.3 assumes 3.1 provides:

* A **snapshot** mount table (read-mostly) usable without heavy locking.
* A fast `mount_by_mountpoint: HashMap<VfsInodeId, MountId>` mapping.
* For each mount:

  * `parent_mount: Option<MountId>`
  * `mountpoint_inode: Option<VfsInodeId>` (inode in parent mount where this mount is attached)
  * `root_inode: VfsInodeId` (inode in this mount representing mount root)
  * `state`/detach semantics for lazy unmount

If 3.1 is not fully implemented yet, 3.3 still defines the required query surface so the two fit cleanly.

---

## Key invariants (must hold after 3.3)

1. **The walker state always includes the current mount.**
   You never have an inode without knowing which mount it belongs to.
2. **Mount transitions happen only based on inode identity**, never by comparing path strings.
3. **Entering a mount occurs after a successful component lookup** that yields an inode that is a mountpoint in the current mount table snapshot.
4. **`..` at mount root returns to parent mountpoint inode** (Linux-like).
   Specifically: if `cwd == mount.root_inode` and you resolve `..`, you end at `parent.mountpoint_inode` in `parent_mount`.
5. **Symlink behavior is unchanged in semantics**, but traversal must be mount-aware when symlink targets are resolved.
6. **Busy or detached mounts**: path walking must not crash or use freed data. A snapshot mount table guarantees safety; semantics for detached mounts must be defined and tested.

---

## API and type design

### 1) `PathWalker` state additions

In `vfs/core/src/path_walker.rs` (or an internal module):

```rust
pub struct WalkerCursor {
    pub mount_id: MountId,
    pub inode: VfsInodeId,        // always belongs to mount_id
    pub node: Arc<dyn FsNode>,    // optional cache; may be lazily loaded
}
```

You may already have a similar struct. Ensure:

* `inode.mount_id == mount_id` is always true
* `node` caching is optional (performance), but correctness must not depend on it

### 2) Mount table query interface required by PathWalker

In `vfs/core/src/mount.rs` (or `mount_table.rs`), provide a read-only snapshot type that the walker can hold:

```rust
pub struct MountTableSnapshot {
    // internal Arc to immutable state
}

impl MountTableSnapshot {
    pub fn lookup_mount_by_mountpoint(&self, mountpoint: VfsInodeId) -> Option<MountId>;

    pub fn get_mount(&self, id: MountId) -> Option<&MountEntryView>;
}

pub struct MountEntryView {
    pub id: MountId,
    pub parent: Option<MountId>,
    pub mountpoint_inode: Option<VfsInodeId>, // None for root mount
    pub root_inode: VfsInodeId,
    pub state: MountState, // Active / Detached
    // maybe flags/namespace if already in 3.1
}
```

**Important:** the snapshot must remain valid for the entire walk. Do not consult a mutable global mount table mid-walk.

### 3) New helper: “apply mount transition”

Add a small pure helper in `path_walker.rs`:

```rust
fn maybe_enter_mount(
    snap: &MountTableSnapshot,
    child: VfsInodeId
) -> VfsInodeId
```

But you usually need to return `(MountId, VfsInodeId)` because the child inode might become the mounted root inode.

So prefer:

```rust
fn maybe_enter_mount(
    snap: &MountTableSnapshot,
    current_mount: MountId,
    child: VfsInodeId,
) -> (MountId, VfsInodeId) {
    if let Some(mount_id) = snap.lookup_mount_by_mountpoint(child) {
        let m = snap.get_mount(mount_id).unwrap();
        // Decide how to treat Detached below.
        return (mount_id, m.root_inode);
    }
    (current_mount, child)
}
```

### 4) New helper: “resolve `..` with mount awareness”

Implement:

```rust
fn resolve_dotdot(
    snap: &MountTableSnapshot,
    cursor: &WalkerCursor,
    // (optional) policy hook from 2.5
) -> VfsResult<WalkerCursor>;
```

Rules:

* If `cursor.inode == mount.root_inode` and mount has a parent:

  * transition to `parent_mount`
  * set inode to `mount.mountpoint_inode` (must exist)
  * node becomes that inode’s node in parent fs
* Otherwise: defer to backend `..` handling:

  * In most designs you already do `lookup("..")` or maintain parent stack
  * For correctness with hardlinks and bind mounts, prefer backend lookup rather than maintaining a path stack (unless Phase 2 already defined a parent stack approach)

---

## Detailed traversal algorithm (component-by-component)

### Inputs to the walker (unchanged, but now mount-aware)

Your `PathWalker::resolve(...)` likely looks like:

* base: `AT_FDCWD` or a directory handle/node/inode
* path: string / `VfsPath`
* flags: follow symlinks / nofollow, allow missing final component (for create), want parent, etc.
* operation context (open/stat/unlink/rename) to drive trailing slash rules and “last component” semantics

**3.3 adds:**

* `mount_snapshot: MountTableSnapshot` captured at the start of resolve

### Step 0: Determine starting cursor (mount + inode)

* If absolute path:

  * start at process namespace root mount’s root inode (often `MountId::ROOT`, but do not hardcode; ask mount table)
* If relative path:

  * start at base FD’s cursor:

    * base FD must carry `VfsInodeId` and `MountId` (from Phase 5 integration later; for now tests can pass a cursor)
  * if base is `AT_FDCWD`, start at thread’s cwd cursor (also mount-aware)

**Acceptance-critical:** base directory may be inside a mounted filesystem; relative traversal must stay inside that mount unless `..` crosses mount root.

### Step 1: Normalize and iterate components

Use your Phase 2 normalization:

* skip empty and `.`
* handle `..` specially (see below)
* maintain trailing slash requirement for last component

### Step 2: For each normal component `name`

1. Ensure current cursor is a directory (or error with correct `ENOTDIR` depending on context and trailing slash rules).
2. Perform backend lookup:

   * `child_inode = current_fs.lookup(current_node, name)` → `VfsInodeId`
3. **Mount enter check**:

   * `(new_mount, new_inode) = maybe_enter_mount(snapshot, cursor.mount_id, child_inode)`
   * Update cursor to `new_mount/new_inode` (and refresh `node` from the *new* mount’s fs)
4. If this is not the last component:

   * If the resolved inode is a symlink and symlink-follow is enabled for intermediate components, resolve it (Phase 2 logic), but **symlink expansion must keep the current cursor’s mount context**.

     * If the symlink target is absolute → restart from namespace root mount root inode (mount-aware).
     * If relative → restart resolution relative to the directory containing the symlink, which might be:

       * the mount root inode (in the mounted filesystem) if the symlink is at mount root
       * a normal directory inode in the current mount otherwise

### Step 3: Handling `..` component

When the component is `..`:

1. If policy (2.5 jail/chroot) forbids escaping a configured root:

   * If the cursor is already at namespace root, keep it there (Linux chroot-ish behavior), or return `EPERM`/`EACCES` depending on your policy model.
     **Pick one rule and document it; do not improvise per-call.**
2. If cursor is at mount root inode **and mount has parent**:

   * cross mount boundary to parent mountpoint inode
3. Else:

   * resolve `..` normally within the backend:

     * usually by lookup `..` and trust backend semantics

**Do not** attempt to cross mount boundaries using backend `..` lookups. That creates inconsistent behavior and breaks the invariant that mount transitions are VFS-driven.

### Step 4: Handling “last component” rules with mounts

All existing Phase 2 last-component rules remain, but ensure they occur *after* mount transitions:

Example: `open("/mnt", O_RDONLY)` where `/mnt` is a mountpoint.

* Lookup `mnt` in parent mount → inode is mountpoint inode
* Enter mount → now inode becomes mounted fs root inode
* Then evaluate “is dir?” / trailing slash rules / open behavior on the mounted root inode.

This matters because mount roots are typically directories; you want behavior consistent with Linux.

### Step 5: Detached mounts / lazy unmount semantics

Mount table has `MountState::{Active, Detached}` per your stub.

Define and implement these rules (simple and testable):

* **Active mount**: normal behavior.
* **Detached mount (lazy unmount / MNT_DETACH)**:

  * The mount is removed from namespace traversal, but existing handles may keep using it.
  * For path walking:

    * If you *enter* a mountpoint inode that maps to a Detached mount in the snapshot, treat it as **not a mountpoint** (i.e., do not enter).
    * If your starting cursor is already inside a detached mount (because a handle/cwd references it), traversal within it continues.
    * `..` at detached mount root:

      * If the mount was detached from namespace, Linux behavior is subtle; for v1, implement:

        * If `parent_mount` is present and `mountpoint_inode` is present, still allow `..` to cross to parent (consistent), **unless** policy forbids it.
      * Add a test and document this explicitly.

This is the least surprising rule set and matches “detached from namespace” rather than “deleted”.

---

## Required changes to existing code structure

### `PathWalker` needs a mount snapshot

Add to `PathWalker`:

```rust
pub struct PathWalker<'a> {
    mount_snapshot: MountTableSnapshot,
    // existing fields: symlink_budget, scratch buffers, flags, etc.
    // maybe references to provider registry / vfs context
}
```

The snapshot is captured from the VFS context at the beginning of each public resolve call:

* `let snap = vfs.mount_table.snapshot();`
* `PathWalker { mount_snapshot: snap, .. }`

### Public resolve results should include mount context

Your resolve output should not be “just a node”. It must include:

* `mount_id`
* `inode`
* (optionally) `node`

Example:

```rust
pub struct ResolvedPath {
    pub mount_id: MountId,
    pub inode: VfsInodeId,
    pub node: Arc<dyn FsNode>,
    pub trailing_slash: bool, // if you already track it
    // optionally parent info, basename, etc. depending on Phase 2 design
}
```

This becomes critical for Phase 5 Wasix integration (“at”-style calls) and Phase 3 overlay composition.

---

## Edge cases to explicitly handle (and test)

### 1) `..` at namespace root

* Must not go above root.
* Policy may clamp to root or deny; pick and document behavior.

### 2) `..` at mount root

* Must return to parent mountpoint inode (not parent directory within the mounted filesystem).

### 3) Symlink targets that cross mountpoints

* `symlink -> "/mnt/x"`: must re-walk from namespace root and enter mounts as needed.
* `symlink -> "../mnt/x"`: must resolve relative to symlink parent directory, and still enter mounts when encountering mountpoints.

### 4) Trailing slash and mount root

* `stat("/mnt/")` where `/mnt` is a mountpoint: should stat the mounted root directory.
* `open("/mnt/", O_RDONLY)` should behave as opening a directory (if supported), otherwise correct `EISDIR`/`EINVAL` per your open rules.

### 5) Cross-mount rename detection (supports Phase 3 acceptance)

Even though “rename across mounts returns EXDEV” is a Phase 3 acceptance criteria, 3.3 should make it easy:

* Provide a helper to resolve both paths and compare `mount_id`.
* If they differ → EXDEV.
  This will be used by Phase 5 Wasix renameat implementation too.

### 6) Busy/detached mount snapshots

* A path walk running concurrently with unmount must remain safe:

  * snapshot means you can still see the mount as active for the duration of that resolve
  * future resolves see it detached/removed

---

## Performance requirements (practical, enforceable)

### Hot-path expectations

* Per component:

  * one backend lookup (already required)
  * one `HashMap` lookup for mount transition (O(1))
* Avoid allocation per component:

  * keep component scratch buffers from Phase 2
  * avoid cloning `Arc`s unnecessarily

### Locking

* PathWalker must not take write locks.
* Snapshot retrieval should be cheap (e.g., `ArcSwap::load_full()` or equivalent).

---

## Tests to add (Phase 3.3-specific)

Create `vfs/core/tests/mount_pathwalk.rs` (or similar). Use `vfs-mem` as backend.

### Test setup utility

* Create a root memfs mount at `/`
* Create directories `/a`, `/mnt`, `/mnt/sub`
* Mount another memfs at `/mnt`

  * That mounted fs has `/sub` and `/file`

### Required test cases

1. **Enter mount**

   * Resolve `/mnt/sub` → should end in mounted fs inode, not parent fs `/mnt/sub`
2. **`..` at mount root**

   * Resolve `/mnt/..` → should return `/` (parent mount), inode = mountpoint inode for `mnt` resolved to parent mountpoint’s parent directory
   * More precise: `/mnt/..` should be `/` if `/mnt` is directly under `/`
3. **Nested path with `..` inside mount**

   * In mounted fs create `/d/e`
   * Resolve `/mnt/d/e/..` → `/mnt/d`
4. **Symlink across mount boundary**

   * In parent fs: `/link -> /mnt/sub`
   * Resolve `/link` → enters mount and reaches mounted `/sub`
5. **Trailing slash behavior**

   * Resolve `/mnt/` for stat-like operation should succeed as directory
   * Resolve `/mnt/file/` should return `ENOTDIR` (file + trailing slash)
6. **Detached mount behavior** (if 3.1 exposes it in tests)

   * Detach mount at `/mnt` and snapshot after detach:

     * resolving `/mnt/sub` should now refer to the parent filesystem’s `/mnt/sub` (or ENOENT if it doesn’t exist there)
   * But if you start from a cursor inside the detached mount (simulate by holding a handle), walking within it still works.

---

## Acceptance criteria for Step 3.3 (hand-off checklist)

* `PathWalker` takes a `MountTableSnapshot` and never reads mutable mount table state mid-walk.
* Entering mounts works for memfs:

  * resolving paths traverses into mounted FS roots based on mountpoint inode identity
* `..` at mount root crosses back to parent mount’s mountpoint inode (Linux-like)
* Symlink resolution still matches Phase 2 semantics and now correctly interacts with mounts
* New tests pass:

  * `cargo test -p vfs-core` includes mount traversal tests
  * At least: enter mount, `..` at mount root, symlink across mount boundary, trailing slash correctness

---

## Implementation notes for junior devs (common pitfalls)

* **Do not compare strings to detect mountpoints.** Always use `VfsInodeId` + mount table map.
* **Always update both mount_id and inode together** when entering/leaving a mount.
* **Be careful when resolving symlinks**: if you “restart” resolution, you must restart with the correct mount/root cursor (namespace root for absolute symlink targets; symlink parent dir for relative targets).
* **Test with nested mounts** early; it’s the easiest way to catch incorrect `..` handling.
* **Don’t hold locks across backend calls**. Snapshot first, then walk using immutable data.

If you want, paste your current `path_walker.rs` (and whatever `MountTable` snapshot API you have so far) and I’ll map this spec directly onto your existing structs/functions with concrete TODOs and suggested function signatures that fit your codebase style.
