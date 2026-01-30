IMPORTANT NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan!!!

## Step 3.4 Spec — Overlay / Union Filesystem (`vfs/overlay`)

### Goal (what 3.4 must deliver)

Implement an **overlay filesystem** that can be mounted like any other `Fs` and that presents a single merged namespace composed of:

* **Upper layer**: read-write
* **Lower layers**: one or more, read-only (treated as RO by overlay even if backend can write)
* **Union view**: “upper shadows lower”, merged directories, copy-up on write, whiteouts, opaque dirs

This overlay must be **Linux-like enough for Wasix rootfs layering**, but deliberately not full Linux overlayfs parity.

---

## How this step fits into the rest of the plan

### Dependencies from earlier steps that overlay must respect

* **Path semantics live in `vfs/core::PathWalker`** (Phase 2): overlay must implement `lookup`, `readlink`, metadata, etc., so the walker can traverse it correctly.
* **Mount transitions are inode-driven** (Step 3.1): overlay must provide **stable `BackendInodeId` values within the overlay mount** so mountpoints inside overlay are reliable.
* **OFD semantics are in `vfs/core::VfsHandle`** (Step 2.4): overlay handles must work with that design; copy-up should preferably happen *before* returning a writable handle to avoid mid-stream surprises.
* **Capability truthfulness** (Phase 1.3): overlay should compute capabilities from layers and fail fast when an operation is impossible.
* **Error mapping centralized in `vfs/unix`**: overlay must return `VfsErrorKind`s that map cleanly to expected `errno`/WASI.

---

## Deliverables

### Code layout

Create `vfs/overlay/` with small modules (no huge files):

* `vfs/overlay/src/lib.rs` – public API, provider registration (if used), `OverlayFs` type
* `vfs/overlay/src/config.rs` – overlay config and builder
* `vfs/overlay/src/fs.rs` – `OverlayFs` implementation (mount root operations)
* `vfs/overlay/src/node.rs` – `OverlayNode` (dir/file/symlink) + lookup/readdir logic
* `vfs/overlay/src/handle.rs` – `OverlayHandle` wrapper (delegation, ensures write always targets upper)
* `vfs/overlay/src/whiteout.rs` – metadata encoding for whiteouts/opaque markers (sidecar naming rules)
* `vfs/overlay/src/copy_up.rs` – copy-up routines (file/dir/symlink + metadata)
* `vfs/overlay/src/inodes.rs` – overlay inode identity mapping table
* `vfs/overlay/src/tests/*` – overlay-specific unit tests; heavier correctness tests can live in `vfs/core/tests` later

---

## Public API surface

### A. Overlay creation

Provide two creation paths (both useful):

#### 1) Direct builder (for tests + internal composition)

```rust
pub struct OverlayBuilder { ... }

impl OverlayBuilder {
    pub fn new(upper: Arc<dyn Fs>, lowers: Vec<Arc<dyn Fs>>) -> Self;
    pub fn with_options(self, opts: OverlayOptions) -> Self;
    pub fn build(self) -> VfsResult<OverlayFs>;
}
```

#### 2) Optional provider registry integration (recommended)

Implement `OverlayProvider` in `vfs/overlay` so `vfs/core` can mount it via provider registry.

**Config design principle:** keep config minimal and explicit; don’t require the layers to already be mounted in the namespace. This avoids confusing “mount overlay on mounts”.

Example config:

```rust
pub struct OverlayConfig {
    pub upper: FsSpec,       // "memfs", "hostfs", ...
    pub lowers: Vec<FsSpec>, // ordered
    pub options: OverlayOptions,
}

pub struct FsSpec {
    pub provider: String,
    pub config: serde_json::Value,
}
```

`OverlayProvider::mount()` constructs each layer FS via registry, then constructs `OverlayFs`, then returns it as the mounted FS instance.

> If your current core provider API isn’t ready for this yet, implement the builder now and stub the provider hookup, but do not block overlay correctness on provider ergonomics.

---

## Overlay semantics: the “contract” (must be implemented + tested)

### 1) Lookup shadowing rules

Given `(parent_dir, name)`:

1. If `name` is a reserved overlay metadata name (see “Reserved names”), pretend it doesn’t exist in the merged view (`ENOENT`).
2. If `upper` contains a real entry `name`, return it.
3. If `upper` has a whiteout for `name`, return `ENOENT` even if lower has it.
4. If parent is **opaque**, do not consult lowers: `ENOENT`.
5. Else search lowers in order; return the first lower layer that contains `name`.

### 2) Directory merge rules

A directory is “merged” when:

* upper has the directory, and
* one or more lowers also have a directory of the same name,
* and the upper directory is **not opaque**

Merged `readdir` order requirements:

* return **upper entries first** (in whatever order upper returns them)
* then for each lower in order, return entries not already returned and not whiteouted
* no sorting required, but results must be deterministic for a fixed backend state

### 3) Whiteouts (tombstones)

Deleting a lower-only entry must hide it in the merged view until overlay is torn down.

* `unlink` or `rmdir` on a lower-only entry **must not mutate lower**
* instead it creates a **whiteout marker** in the *upper directory*
* whiteouts must suppress all lower layers, not just the first match

### 4) Opaque directories

An upper directory can be marked opaque so it hides *all* lower entries under that directory.

* Opaque marker is stored in upper directory metadata (sidecar file)
* If opaque marker present:

  * merged readdir returns only upper entries
  * lookup never consults lower dirs under that directory
* Opaque marker must be preserved across operations unless explicitly cleared (you can omit “clear” API in v1; just support creating it when needed)

### 5) Copy-up on write intent

Overlay must copy-up before the first operation that mutates lower-only content.

**v1 rule (required):** copy-up happens **at open time** when write intent is present:

* `O_WRONLY`, `O_RDWR`, `O_TRUNC`, `O_CREAT` imply write intent
* if target resolves to lower-only node, copy it up before returning handle

Also required: metadata mutation on lower-only should copy-up first:

* `chmod/chown/utimens`, `truncate`, `link`, `rename` (where supported)

### 6) Rename semantics

* Cross-mount rename is handled by mount layer → `EXDEV` (not overlay’s job)
* Within an overlay mount:

  * File rename supported

    * If source is lower-only file: copy-up source, then rename in upper
    * Directory rename where source exists only in lower: **ENOTSUP** (explicitly tested)
  * Rename that replaces a lower-only destination must ensure the replaced lower entry stays hidden afterward (see whiteout strategy below)

---

## Overlay metadata persistence scheme (whiteouts + opaque)

### Requirements this scheme must satisfy

* Works on upper backends without xattrs
* Confinement-safe (no string prefix hacks)
* Survives restart **only if upper is persistent** (hostfs); memfs doesn’t need persistence but should still use same logical model
* Not visible through the overlay merged view

### Chosen scheme: sidecar files inside each upper directory

In each upper directory, maintain “overlay metadata” as hidden files:

* **Opaque marker file:** `.wasmer_overlay.opaque`
* **Whiteout marker file for entry `NAME`:** `.wasmer_overlay.wh.NAME`

Notes:

* These files exist *in upper layer only*.
* Overlay must **filter them out** of readdir results and treat them as “internal”.
* When checking if `NAME` is whiteouted: lookup for `.wasmer_overlay.wh.NAME` in upper directory.

#### Reserved names policy (must implement)

To prevent user spoofing overlay internal files:

* Disallow create/rename/link/symlink operations that attempt to create a name starting with:

  * `.wasmer_overlay.`

Return `EACCES` or `EINVAL` (pick one and document; recommended `EACCES` as “policy denied”).

---

## Inode identity (critical for mount table correctness)

### Why this matters

Step 3.1’s mount table transitions are keyed by `VfsInodeId` of mountpoints. If overlay changes inode IDs when copy-up happens, mountpoints inside overlay could break.

### Overlay inode strategy (required)

Overlay must present a **stable `BackendInodeId` namespace for the overlay mount** that is independent of the underlying layer’s inode changing due to copy-up.

#### Practical design

Implement an `OverlayInodeTable` that assigns and maintains overlay backend IDs:

* `OverlayBackendInodeId: u64` (monotonic counter)
* Mapping from **underlying identity** → overlay identity, but with a “promotion” ability:

  * If a node starts in lower, then gets copied-up into upper, it must retain the same overlay ID.

Recommended data model:

```rust
enum BackingKey {
    Upper { upper_inode: BackendInodeId },
    Lower { layer: u16, lower_inode: BackendInodeId },
    // Optional: VirtualMergedDir { path_hash or (upper+lower set) } if needed.
}

struct OverlayInodeTable {
    next: AtomicU64,
    key_to_overlay: RwLock<HashMap<BackingKey, BackendInodeId>>,
    overlay_to_backing: RwLock<HashMap<BackendInodeId, BackingState>>,
}

struct BackingState {
    // current authoritative backing (upper if present, else some lower)
    primary: BackingKey,
    // for merged dirs: list of additional lower dir backings
    lowers: SmallVec<[BackingKey; 2]>,
}
```

Promotion rule:

* If a node’s overlay ID was assigned while it was lower-backed, and later it is copy-upped, update `overlay_to_backing[overlay_id].primary` to `Upper{...}` **without changing `overlay_id`**.

For directories:

* If upper directory exists, treat it as primary.
* If only lower exists, primary is the first lower dir found.
* If later upper dir is created (copy-up ancestors), promote.

> This gives mount table stable mountpoints and keeps `stat().ino` stable across copy-up for the same file within overlay lifetime.

---

## Core types in `vfs/overlay`

### `OverlayFs`

Holds the layers and shared tables.

```rust
pub struct OverlayFs {
    upper: Arc<dyn Fs>,
    lowers: Vec<Arc<dyn Fs>>,
    opts: OverlayOptions,

    // identity + metadata helpers
    inodes: Arc<OverlayInodeTable>,
}
```

`OverlayOptions` (v1 minimal set):

* `max_copy_chunk_size: usize` (default e.g. 128 KiB)
* `deny_reserved_names: bool` (default true)
* `lower_readonly: bool` (default true; always true in v1)
* (Optional) `metadata_mode: OverlayMetadataMode` (but keep it simple; sidecar files are fine)

### `OverlayNode`

Represents a node in the overlay view.

```rust
pub enum OverlayNodeKind {
    File,
    Dir,
    Symlink,
}

pub struct OverlayNode {
    fs: Arc<OverlayFs>,
    kind: OverlayNodeKind,

    // overlay identity
    overlay_inode: BackendInodeId,

    // backing handles (upper optional; lowers optional)
    upper: Option<Arc<dyn FsNode>>,
    lowers: SmallVec<[Arc<dyn FsNode>; 2]>, // for merged dirs, or single lower backing
}
```

Rules:

* For **file/symlink**: at most one backing node is “active” (upper if exists else first lower)
* For **dir**: allow upper + multiple lower dir backings for merge

### `OverlayHandle`

A thin wrapper that delegates IO to the underlying handle.

Rule:

* Writable handles must always be opened from **upper**.
* Read-only handles may come from lower.

---

## Algorithms (what junior dev implements)

### A) `OverlayNode::lookup(name)`

1. Validate `name` (reject reserved name creation paths elsewhere; for lookup just hide internal files)
2. If `upper` exists:

   * If `upper` has real child `name`: return child (even if whiteout exists)
   * If whiteout marker exists: return `ENOENT`
3. If current dir is opaque: return `ENOENT`
4. Search lowers in order:

   * If found: return lower child
5. Else `ENOENT`

To compute “real child exists in upper” vs “whiteout marker exists”, implement helpers in `whiteout.rs`:

* `fn whiteout_name_for(entry: &str) -> String`
* `fn is_opaque_marker(name: &str) -> bool`
* `fn is_whiteout_marker(name: &str) -> Option<&str>` (extract original)

### B) `OverlayNode::read_dir()`

1. Read upper entries if upper exists:

   * Filter out `.wasmer_overlay.*` entries
   * Keep a `seen_names` set of real upper entry names
   * Keep a `whiteouts` set derived from `.wasmer_overlay.wh.*` entries
   * Yield real upper entries in the same order returned
2. If opaque marker present in upper dir: stop
3. For each lower dir backing in order:

   * read entries
   * for each entry:

     * skip if name in `seen_names`
     * skip if name in `whiteouts`
     * skip if name is internal reserved (shouldn’t exist in lower, but keep it consistent)
     * insert into `seen_names`
     * yield

### C) Copy-up logic (`copy_up.rs`)

Provide:

* `copy_up_file(parent_upper_dir, name, lower_file_node) -> upper_file_node`
* `copy_up_symlink(...)`
* `ensure_upper_dir(path/ancestors)` (but prefer handle-based traversal: “ensure upper parent dir exists” using nodes, not string paths)

**Copy-up file details (v1):**

* Create file in upper with same name (fail if upper already has it)
* Copy metadata (mode, uid/gid if meaningful, times if supported)
* Copy data by streaming:

  * open lower read handle
  * open upper write handle
  * loop read chunks and write chunks
* If lower is huge, do not buffer whole file

### D) Mutation operations

Implement overlay ops by translating them into upper operations + metadata markers.

#### `unlink(name)` in directory

Case analysis:

* If upper has entry:

  * remove upper entry
  * if any lower has entry with same name: create whiteout marker
* Else if lower has entry:

  * create whiteout marker
* Else: `ENOENT`

#### `rmdir(name)`

Same, but validate emptiness via overlay semantics:

* If upper dir exists: ensure directory is empty in overlay view before removing (standard `ENOTEMPTY`)
* If only lower dir exists: you can’t delete lower; create whiteout marker (but still must ensure it’s empty? This is a policy choice)

  * Recommended v1: require directory to be empty in the merged view before allowing rmdir, else `ENOTEMPTY`.

#### `create/mkdir/symlink`

Always operate in upper:

* Ensure upper parent dir exists (copy-up ancestors as needed)
* If whiteout marker exists for that name, **keep it** (it records deletion of lower), but do not let it hide the new upper entry:

  * your lookup logic already prefers upper real entry before consulting whiteout

#### `rename(old, new)`

Required behavior:

* If source is lower-only **file**: copy-up source to upper first
* If source is lower-only **dir**: return `ENOTSUP`
* Perform rename in upper between the corresponding upper paths/nodes (handle-based if possible)
* If destination existed in lower but not upper: create a whiteout marker for destination *after the rename*, so if the new upper file is later removed, the old lower dest does not reappear.

> This requires your whiteout markers to be *separate files* (as specified), not “same-name special nodes”.

---

## Capability model for overlay

Overlay capabilities should be derived conservatively:

* `READ` always if at least one layer readable
* `WRITE` only if upper supports it
* `SYMLINK`, `HARDLINK`, `RENAME_ATOMIC`, etc.:

  * if operation requires support in upper, gate on upper capability
  * if it requires reading from lower too, gate on lower read capability
* If unsupported, return `VfsErrorKind::NotSupported` (maps to `ENOTSUP`)

---

## Concurrency / locking expectations

Overlay is on the hot path; keep locks coarse but not global-contentious:

* `OverlayInodeTable` uses `RwLock` internally; reads are frequent.
* Avoid per-operation allocation; reuse smallvecs.
* `read_dir` should avoid quadratic behavior; use a hash set for `seen_names`.

---

## Integration points with other phases

### With mount table (3.1)

* Overlay runs as its own mounted `Fs`.
* PathWalker sees overlay’s `VfsInodeId` like any other.
* Stable overlay inode IDs are required so “mount inside overlay” works.

### With PathWalker (2.1 + 3.3)

* PathWalker handles symlink traversal; overlay must:

  * correctly report node types
  * implement `readlink` for symlink nodes
  * produce `ENOTDIR`, `ELOOP`, etc. via correct `VfsErrorKind`s

### With handle semantics (2.4)

* Writable opens must return upper-backed handles (copy-up before open).
* Read-only opens may return lower-backed handles.

### With Wasix (Phase 5)

* Overlay must behave predictably for rootfs layering:

  * merged `readdir`
  * copy-up on write
  * deletions hide lower entries
  * opaque dirs hide lower subtrees

---

## Tests (must be written as part of 3.4)

Write tests against `vfs-mem` for upper and lower layers first (fast + deterministic). If hostfs exists later, add a small integration test.

### Required test cases

1. **Upper shadows lower**

   * lower has `/a`, upper has `/a` → read returns upper content
2. **Merged readdir**

   * upper dir entries show first, then lower entries not shadowed
3. **Whiteout hides lower**

   * lower has `/x`; unlink `/x` in overlay → lookup `/x` returns ENOENT, readdir doesn’t show it
4. **Opaque directory hides all lower**

   * lower has `/d/file`; upper marks `/d` opaque → `/d/file` ENOENT
5. **Copy-up on open for write**

   * lower has `/f` content; open `/f` with write intent; verify upper now has `/f` and modifications don’t affect lower
6. **Rename lower-only file supported**

   * lower `/f` → rename to `/g` succeeds, `/g` exists in upper, `/f` ENOENT
7. **Rename lower-only directory ENOTSUP**

   * lower `/dir` rename `/dir` → `/dir2` returns `ENOTSUP`
8. **Mountpoint stability smoke test (overlay inode stability)**

   * Resolve inode id for `/mp`
   * Trigger copy-up of `/mp` (e.g., metadata op or create upper dir) and ensure inode id remains the same for that entry **within overlay lifetime** (or at least that lookups return the same overlay id)
   * This is crucial for Step 3.1 assumptions

---

## Phase 3 acceptance criteria additions for overlay

Overlay portion of Phase 3 acceptance criteria should be concretely satisfied by:

* `cargo test -p vfs-overlay` passes
* Basic copy-up + whiteout + merged readdir tests exist and pass
* Overlay inode IDs remain stable across copy-up for the same node (at least for directories and ordinary files) so mount table semantics are not invalidated

---

## Explicit non-goals for v1 (must not accidentally creep in)

* metacopy (metadata-only copy-up without copying file data)
* redirect_dir, index, NFS export semantics
* full Linux overlayfs inode parity across complex rename + open-handle edge cases
* xattr/ACL merge semantics beyond “pass through upper if present”

---

## Implementation checklist (junior-friendly)

1. Create crate + modules listed above.
2. Implement `whiteout.rs` naming/filtering logic.
3. Implement `OverlayInodeTable` with promotion semantics.
4. Implement `OverlayFs::root()` returning an `OverlayNode` for root.
5. Implement `OverlayNode::lookup` + `read_dir` using the exact merge algorithm.
6. Implement copy-up routines (file + symlink + ensure-parent-dir-in-upper).
7. Implement mutations: create/mkdir/unlink/rmdir/rename with whiteouts + copy-up rules.
8. Add the required tests (start with memfs layers).
9. Ensure errors use correct `VfsErrorKind` so `vfs/unix` maps them predictably.

If you want, I can also draft a concrete Rust type skeleton for `OverlayFs/OverlayNode/OverlayInodeTable` that matches the *current* `vfs/core` traits you already have in-tree (so it plugs in cleanly without refactors).
