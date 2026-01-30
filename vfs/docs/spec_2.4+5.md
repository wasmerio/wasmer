
Below is a **junior-dev-implementable, detailed spec** for **Phase 2 / Step 2.4 (handles + OFD)** and **Step 2.5 (metadata + permission model)**, written to fit the plan’s layering contract and to remain compatible with later phases (mounts/overlay, sync+async traits, Wasix FD table integration, hostfs confinement, etc.).


NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan.

---

# Step 2.4 — File handles and Open File Descriptions (OFD)

## 2.4.0 Goals and invariants

### Goals

Implement **Linux-like Open File Description (OFD)** behavior in `vfs-core`:

* **One OFD = shared open-file state**:

  * Shared file offset
  * Shared “status flags” that are OFD-scoped (e.g. `O_APPEND`, `O_NONBLOCK` if you choose it to be OFD-scoped in VFS)
  * Shared reference to the underlying backend handle object
* **Multiple FDs can reference the same OFD** (dup / fork semantics), and **share offset**.
* Provide **positionless backend IO** (`read_at`/`write_at`) so OFD offset management is in `vfs-core`, not in the backend.
* Keep it **fast** on hot paths: minimal allocations, avoid global locks, per-handle locking only when needed.

### Non-goals (explicitly deferred)

* Real POSIX record locks / flock semantics (can be added later).
* `O_DIRECT`, `O_DSYNC` precise behavior (store flags, pass down if backend supports).
* Full `fcntl(F_SETFL)` behavior in core (Wasix will likely implement FD flags; VFS may expose an OFD flag mutator API but can defer complex semantics).

### Critical compatibility constraints (from the plan)

* **Wasix resource table owns FD-like state** (e.g. `CLOEXEC`), and must not mutate OFD state inadvertently.
* **All path resolution remains in `PathWalker`**; `handle.rs` must not accept raw absolute strings to “re-open” things.
* **Backends must not rely on global paths** (hostfs uses openat-style confinement; memfs uses inode tables).
* **Cross-mount rename returns `EXDEV`** (Step 3.x), but **handles must carry enough identity** (`VfsInodeId`) to enforce cross-mount rules later.

---

## 2.4.1 File layout and module responsibilities

**Deliverable file:** `vfs/core/src/handle.rs`

Recommended internal submodules to keep the file small:

* `handle.rs` (public types + core logic)
* `handle/state.rs` (OFD shared state)
* `handle/flags.rs` (flag types and conversions)
* `handle/ops.rs` (read/write/seek implementations)
* `handle/tests.rs` (unit tests; integration tests can go to `vfs/core/tests/`)

---

## 2.4.2 Core types

### 2.4.2.1 IDs and lightweight handles

These IDs are used for debugging and future tables/caches, **not required** for correctness of dup sharing.

```rust
pub type VfsHandleId = u64; // or NonZeroU64
```

You can generate `VfsHandleId` via an `AtomicU64` counter in `vfs-core` (per VFS context is better than global; see below).

### 2.4.2.2 VfsHandle (the OFD object)

`VfsHandle` is the **OFD**. It should be **cheap to clone** (Arc) so Wasix can share it across duplicated FDs.

```rust
pub struct VfsHandle {
  id: VfsHandleId,
  inode: VfsInodeId,          // from step 2.2
  file_type: VfsFileType,     // cached at open time if available
  access: HandleAccess,       // read/write rights at VFS layer
  ofd: std::sync::Arc<OFDStateSync>,
  // optionally: async handle pointer too, see below
}
```

* `inode` is used later for mount/overlay rules, watcher registration, and metadata caching.
* `file_type` helps fast-path `IsDir` errors for read/write if the backend provides it at open time.
* `access` is derived from open flags (readable/writable) and **WASI rights gating happens before** VFS, but VFS still needs to enforce read-only opens internally.

### 2.4.2.3 OFDStateSync (shared, mutable state)

This holds shared state. It must support:

* atomic-ish offset updates
* status flags updates
* atomic append semantics (serialize seek-to-end+write when backend can’t do it atomically)

```rust
pub struct OFDStateSync {
  // Underlying backend handle (positionless IO)
  backend: std::sync::Arc<dyn FsHandleSync>,

  // OFD-scoped flags that affect IO behavior
  status_flags: std::sync::atomic::AtomicU32, // bitflags packed

  // Shared offset
  offset: std::sync::atomic::AtomicU64,

  // Serialize operations that require compound atomicity
  // (append emulation, read+offset update, write+offset update)
  io_lock: parking_lot::Mutex<()>,
}
```

**Why both `AtomicU64` and `io_lock`?**

* Atomics make `tell()` fast and allow some operations to avoid taking a lock.
* The `io_lock` ensures correctness for:

  * “read from current offset then increment offset”
  * “write at current offset then increment offset”
  * “append: compute end offset + write at end” (must be atomic w.r.t. other writers on same OFD)

> Use `parking_lot` for lower overhead if allowed by workspace policy; otherwise `std::sync::Mutex`.

### 2.4.2.4 Sync + Async coexistence

Step 2.4 happens before Phase 4, but your types must not block Phase 4.

**Rule:** `VfsHandle` must be able to expose both sync and async IO paths, but you can implement only sync now and add async glue later.

Recommended approach:

* Keep `backend: Arc<dyn FsHandleSync>` in step 2.4.
* In Phase 4, introduce `FsHandleAsync` and adapters. Don’t redesign `VfsHandle`; just add an async accessor or wrapper.

---

## 2.4.3 Backend handle trait contract (required by 2.4)

Even though the plan places traits in Phase 4, Step 2.4 needs a minimal contract so memfs can work.

Add (temporary or final) trait in `vfs/core/src/node.rs` or `vfs/core/src/handle.rs` (choose one place and stick to it):

```rust
pub trait FsHandleSync: Send + Sync {
  fn read_at(&self, offset: u64, dst: &mut [u8]) -> VfsResult<usize>;
  fn write_at(&self, offset: u64, src: &[u8]) -> VfsResult<usize>;

  fn flush(&self) -> VfsResult<()>;
  fn fsync(&self) -> VfsResult<()>;

  fn get_metadata(&self) -> VfsResult<VfsMetadata>; // or a lighter stat struct
  fn set_len(&self, new_len: u64) -> VfsResult<()>;

  // Optional but helpful:
  fn is_seekable(&self) -> bool { true }
}
```

**Important:** `read_at`/`write_at` must be **positionless** w.r.t. backend state. For hostfs, that implies `pread/pwrite`-like behavior internally (later). For memfs, it’s trivial.

---

## 2.4.4 Public API on VfsHandle

### 2.4.4.1 Basic info

```rust
impl VfsHandle {
  pub fn id(&self) -> VfsHandleId;
  pub fn inode(&self) -> VfsInodeId;
  pub fn status_flags(&self) -> HandleStatusFlags;
  pub fn set_status_flags(&self, new_flags: HandleStatusFlags) -> VfsResult<()>;
}
```

**Notes:**

* `set_status_flags` is needed later for `fcntl(F_SETFL)`-like behavior.
* You must define which flags are allowed to change at runtime (typically `O_APPEND`, `O_NONBLOCK`, maybe `O_SYNC` if supported). Disallow changing read/write mode.

### 2.4.4.2 Offset control

```rust
impl VfsHandle {
  pub fn tell(&self) -> u64;
  pub fn seek(&self, pos: std::io::SeekFrom) -> VfsResult<u64>;
}
```

**Seek semantics**

* Validate negative resulting offsets -> `VfsError::InvalidInput`.
* `SeekFrom::End(n)` requires file size; get it via `backend.get_metadata()` and use `size` as end.
* If file is not seekable, return `VfsError::NotSupported` (or `InvalidInput` depending on how you want it mapped; prefer `NotSupported`).

### 2.4.4.3 Sequential read/write (uses OFD offset)

```rust
impl VfsHandle {
  pub fn read(&self, dst: &mut [u8]) -> VfsResult<usize>;
  pub fn write(&self, src: &[u8]) -> VfsResult<usize>;
}
```

**Correctness requirements**

* Both functions must:

  1. Check access mode (readable/writable)
  2. Take `io_lock`
  3. Load current offset
  4. Perform backend IO at that offset (or append logic below)
  5. Update offset by bytes transferred
* For short reads/writes, update by actual count.

**Append behavior**

* If `O_APPEND` is set:

  * Under `io_lock`, do:

    * `end = backend.get_metadata()?.size`
    * `n = backend.write_at(end, src)`
    * update offset to `end + n`
  * This yields correct per-OFD atomic append. (Cross-process atomicity is a backend/OS feature; VFS can only guarantee within the same OFD.)

### 2.4.4.4 Positioned IO (does not change OFD offset)

```rust
impl VfsHandle {
  pub fn pread_at(&self, offset: u64, dst: &mut [u8]) -> VfsResult<usize>;
  pub fn pwrite_at(&self, offset: u64, src: &[u8]) -> VfsResult<usize>;
}
```

These bypass `io_lock` unless you need append emulation (append shouldn’t apply here; `pwrite` ignores `O_APPEND` on Linux? In POSIX, `pwrite` writes at given offset and does not change file offset. Implement that: ignore append.)

### 2.4.4.5 Truncate and sync

```rust
impl VfsHandle {
  pub fn set_len(&self, new_len: u64) -> VfsResult<()>;
  pub fn flush(&self) -> VfsResult<()>;
  pub fn fsync(&self) -> VfsResult<()>;
}
```

**Truncate semantics**

* Requires write permission or a capability (align with POSIX-ish).
* If current offset > new_len, Linux keeps offset? Actually file offset is not automatically clamped on truncate for existing open file descriptions in all cases; behavior varies, but common is that offset remains and later reads return EOF. For simplicity (and compatibility with many expectations), **do not modify offset** on truncate.

---

## 2.4.5 Flag model: what lives in VFS vs Wasix

### 2.4.5.1 VFS internal flag types

Define two separate flag sets:

1. **Open flags**: used at open-time to decide creation/truncation/access.

```rust
bitflags::bitflags! {
  pub struct OpenFlags: u32 {
    const READ = 1<<0;
    const WRITE = 1<<1;
    const CREATE = 1<<2;
    const TRUNC = 1<<3;
    const EXCL = 1<<4;
    const DIRECTORY = 1<<5;
    const NOFOLLOW = 1<<6;
    // etc.
  }
}
```

2. **Status flags** (OFD-scoped behavior during IO):

```rust
bitflags::bitflags! {
  pub struct HandleStatusFlags: u32 {
    const APPEND = 1<<0;
    const NONBLOCK = 1<<1;
    const SYNC = 1<<2;
    // etc.
  }
}
```

**Mapping responsibility**

* WASI/WASIX ↔ internal flags mapping must be centralized in `vfs-unix` later.
* For step 2.4, define internal flags and leave conversions minimal or test-only.

### 2.4.5.2 What must *not* be in VfsHandle

* `CLOEXEC` is per-FD, stored in Wasix resource table only.
* Anything that only affects descriptor behavior (dup2 target flags, etc.) stays in Wasix.

---

## 2.4.6 Construction path: how VfsHandle is created

`VfsHandle` must be created by **`FsNode::open`** (from Step 2.3) or by a small helper in `vfs-core` that wraps the backend handle.

Expected open flow (later integrated with `PathWalker`):

1. `PathWalker` resolves to `(mount, inode, node)`
2. Permission checks (Step 2.5) happen before open when meaningful
3. Backend `node.open(flags)` returns `Arc<dyn FsHandleSync>`
4. VFS wraps into `VfsHandle { ofd: Arc<OFDStateSync> }`

**O_TRUNC and O_CREAT**

* These semantics belong in `vfs-core` open logic (not inside `VfsHandle`), but step 2.4 must support `set_len` and metadata queries so open can implement them:

  * If `TRUNC` set and file exists and opened writable ⇒ call `set_len(0)` after open succeeds.

---

## 2.4.7 Concurrency rules

* `VfsHandle` is `Clone` and clones share the same `Arc<OFDStateSync>`.
* `read` and `write` must be linearizable w.r.t. offset updates for the same OFD:

  * enforce via `io_lock`.
* `pread_at/pwrite_at` are independent; they may run concurrently with sequential read/write.

---

## 2.4.8 Tests (must exist by end of step 2.4)

Place core-level tests in `vfs/core/tests/handle_ofd.rs` using memfs.

**Test cases**

1. **dup shares offset**

* open file, write “hello” with handle A, seek 0, clone handle as B
* read 2 bytes via A (offset becomes 2)
* read next 2 bytes via B; must read starting at offset 2

2. **pread does not change offset**

* seek 4
* pread_at(0, 2 bytes) returns bytes from start
* tell() still 4

3. **append is atomic within an OFD**

* create file, set APPEND flag
* spawn threads (or loop) doing writes on cloned handles
* resulting file size equals sum; and each write content appears as intact chunks (no overlapping). (You don’t need global ordering, just non-overlap.)

4. **seek end works**

* write N bytes, seek end(-k), read… etc.

---

# Step 2.5 — Metadata and permission model

## 2.5.0 Goals and invariants

### Goals

Implement:

* A consistent `VfsMetadata` model sufficient for POSIX-ish `stat` and for permission checks.
* A **capability-aware** and **policy-pluggable** permission system:

  * Works fully for memfs (POSIX-like)
  * Works best-effort for hostfs (later) and non-POSIX backends (capability gated)
* A clear, enforceable **evaluation order** aligned with plan:

  1. Wasix rights checks (outside VFS)
  2. VFS permission + mount policy checks (this step)
  3. Backend operation
  4. Error mapping centralized in `vfs-unix`

### Non-goals (explicitly deferred)

* ACLs and extended attributes.
* Full Linux capability bits / setuid/setgid semantics (you can store bits in mode but don’t implement privilege escalation).
* Complex user namespace / mount namespace semantics (mount table comes later; this step provides hooks).

---

## 2.5.1 File layout and module responsibilities

**Deliverable file:** `vfs/core/src/metadata.rs`

Recommended submodules:

* `metadata.rs` (public structs/enums)
* `metadata/mode.rs` (mode bits helpers)
* `metadata/policy.rs` (policy traits + default impl)
* `metadata/tests.rs`

---

## 2.5.2 Metadata types

### 2.5.2.1 Core scalar types

```rust
pub type VfsUid = u32;
pub type VfsGid = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsFileMode(pub u32); // lower 12 bits typical; keep u32 for portability
```

### 2.5.2.2 File type enum

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsFileType {
  Regular,
  Directory,
  Symlink,
  CharDevice,
  BlockDevice,
  Fifo,
  Socket,
  Unknown,
}
```

### 2.5.2.3 Timestamps

Use a simple, explicit representation (avoid `SystemTime` pitfalls across WASI/host). Example:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfsTimestamp {
  pub secs: i64,
  pub nanos: u32,
}
```

### 2.5.2.4 VfsMetadata

```rust
#[derive(Clone, Debug)]
pub struct VfsMetadata {
  pub inode: VfsInodeId,        // mount-scoped identity
  pub file_type: VfsFileType,

  pub mode: VfsFileMode,        // includes type bits if you want, or store separately
  pub nlink: u64,

  pub uid: VfsUid,
  pub gid: VfsGid,

  pub size: u64,

  pub atime: VfsTimestamp,
  pub mtime: VfsTimestamp,
  pub ctime: VfsTimestamp,

  // Device info for special files (optional for now)
  pub rdev_major: u32,
  pub rdev_minor: u32,
}
```

**Backend capability reality**

* Some backends won’t provide uid/gid/nlink/ctime. For those:

  * Fill with mount-config defaults, and record “incomplete” via capability flags if needed.
  * Permission checks can be configured to either:

    * enforce based on stored values (memfs), or
    * bypass/permit (hostfs may defer to OS), depending on policy.

---

## 2.5.3 Permission checking: the model

### 2.5.3.1 Credentials passed into VFS ops

All VFS operations that may require permission checks must accept a `VfsCred`.

```rust
pub struct VfsCred {
  pub uid: VfsUid,
  pub gid: VfsGid,
  pub groups: smallvec::SmallVec<[VfsGid; 4]>,
}
```

Where does this come from?

* From Wasix context / runtime configuration. If Wasix doesn’t track real uid/gid today, it can supply a fixed `(0,0)` or `(1000,1000)` — but the API must exist now so semantics can be enforced later.

### 2.5.3.2 Access request enum

```rust
bitflags::bitflags! {
  pub struct VfsAccess: u8 {
    const READ = 1<<0;
    const WRITE = 1<<1;
    const EXEC = 1<<2; // “search” permission on directories
  }
}
```

### 2.5.3.3 POSIX mode evaluation (core logic)

Implement:

* owner/group/other permission bits
* group membership check (`cred.gid` or in `cred.groups`)
* uid == 0 special-casing: **decide explicitly**:

  * Minimal viable: treat uid 0 as bypass (common in many systems).
  * If you want stricter WASI behavior, make it policy-driven.

**Directory semantics**

* Traversal (`PathWalker` stepping through directories) requires **EXEC** (“search”) on each traversed directory when permission enforcement is enabled.
* `readdir` requires READ on directory (and usually EXEC too; Linux often requires EXEC to access entries; implement READ+EXEC for simplicity and safety).

### 2.5.3.4 Policy hooks (per-mount)

You need a mount-level policy system because:

* hostfs may want to rely on OS permissions
* sandbox/jail rules may override POSIX checks
* object stores can’t do real perms

Define:

```rust
pub trait VfsPolicy: Send + Sync {
  fn check_path_component_traverse(
    &self,
    cred: &VfsCred,
    dir_meta: &VfsMetadata,
  ) -> VfsResult<()>;

  fn check_open(
    &self,
    cred: &VfsCred,
    node_meta: &VfsMetadata,
    open_flags: OpenFlags,
  ) -> VfsResult<()>;

  fn check_mutation(
    &self,
    cred: &VfsCred,
    parent_dir_meta: &VfsMetadata,
    op: VfsMutationOp,
  ) -> VfsResult<()>;
}
```

And mutation op:

```rust
pub enum VfsMutationOp {
  CreateFile { name: String },
  CreateDir { name: String },
  Remove { name: String, is_dir: bool },
  Rename { from: String, to: String },
  Link { name: String },
  Symlink { name: String },
  SetMetadata { what: SetMetadataWhat },
}
```

**Default policy**

* `PosixPolicy { enforce: bool, root_bypass: bool }`

  * If `enforce=false`, all checks succeed (useful for early bring-up and non-POSIX backends).
  * For memfs tests, set `enforce=true`.

**Where policy lives**

* In the per-mount `Fs` instance or mount table entry (Phase 3).
* For Phase 2, store it in a `VfsContext` passed into operations, or embed it in the `Fs` wrapper created by the provider.

---

## 2.5.4 Metadata operations API (VFS-facing)

### 2.5.4.1 Get metadata

You need consistent stat behavior for:

* `PathWalker` (dir traverse checks)
* `open` checks (file type, perms)
* Wasix syscalls later

Define VFS-level methods (on nodes and handles):

* `FsNode::get_metadata(follow_symlinks: bool)` (node-level)
* `FsHandle::get_metadata()` (handle-level; already in 2.4 trait)

**Symlink behavior**

* `lstat` corresponds to `follow_symlinks=false` on the final path component.
* `stat` corresponds to `follow_symlinks=true`.

PathWalker controls this; metadata layer just exposes both.

### 2.5.4.2 Set metadata

Define a single struct describing requested changes:

```rust
pub struct VfsSetMetadata {
  pub mode: Option<VfsFileMode>,
  pub uid: Option<VfsUid>,
  pub gid: Option<VfsGid>,
  pub atime: Option<VfsTimestamp>,
  pub mtime: Option<VfsTimestamp>,
}
```

Backends implement what they can; VFS gates via capabilities:

* If `CHOWN` capability absent and uid/gid requested → `VfsError::NotSupported`
* If `UTIMENS` capability absent and times requested → `NotSupported`
* If `CHMOD` capability absent and mode requested → `NotSupported`

Even for memfs, implement all.

---

## 2.5.5 Integration points with other steps

### With 2.1 PathWalker

* PathWalker must call policy checks on each directory during traversal when enforcement is enabled:

  * fetch dir metadata
  * `policy.check_path_component_traverse(cred, &dir_meta)`
* When `NOFOLLOW` or `AT_SYMLINK_NOFOLLOW` is in effect, PathWalker must:

  * stop before following the symlink and return the symlink node/metadata

### With 2.3 FsNode interface

* `lookup` and `read_dir` should return entries + (optionally) metadata.
* If lookup doesn’t return metadata, PathWalker will call `get_metadata` on the resulting node for permission checks.

### With 2.4 VfsHandle

* `open` must run `policy.check_open` before creating `VfsHandle`.
* IO operations may optionally run permission checks, but that’s usually redundant after open; rely on open-time checks unless you plan to support dynamic permission changes.

### With Phase 3 mounts

* Policy must be per-mount, because different mounts may enforce differently.
* `VfsInodeId` includes `MountId`; metadata must preserve that identity.

### With overlay (Phase 3.4)

* Overlay must decide how to present metadata:

  * For v1, simplest: metadata comes from the chosen layer node (upper if present, else lower).
  * Permissions should be checked against the visible node’s metadata.
  * Copy-up will bring metadata into upper; after copy-up, checks naturally apply to upper metadata.

### With Wasix integration (Phase 5)

* Wasix does rights gating first.
* Wasix passes `VfsCred` and open flags into VFS.
* Wasix maps `VfsError::PermissionDenied` etc. to `wasi::Errno` via `vfs-unix` only.

---

## 2.5.6 Error model requirements

`metadata.rs` and permission checks must use core `VfsError` variants (from Phase 1/2 core types). At minimum:

* `PermissionDenied`
* `NotSupported`
* `InvalidInput`
* `NotFound`
* `NotDir`
* `IsDir`

**No direct `wasi::Errno` mapping here**.

---

## 2.5.7 Tests (must exist by end of step 2.5)

Put in `vfs/core/tests/metadata_perms.rs` using memfs.

**Required tests**

1. **Traverse requires exec**

* Create `/a` as dir with mode `000` for others, owned by uid 1000
* Use `cred uid=2000` try to resolve `/a/b` → must fail `PermissionDenied` at traversal

2. **Open requires read/write bits**

* File owned by uid 1000, mode `0400`
* `cred uid=2000` open for read → denied
* `cred uid=1000` open for read → allowed

3. **Create requires write+exec on parent**

* Dir `/d` mode `0555` → create file inside denied
* Dir `/d` mode `0755` owned by same uid → allowed

4. **chmod affects later opens**

* Create file mode `0600` owned by uid 1000
* chmod to `0644`
* `cred uid=2000` open read allowed afterwards

---

# Acceptance criteria for completing 2.4 + 2.5

By the end of these steps:

### Build

* `cargo test -p vfs-core`
* `cargo test -p vfs-mem`

### Functional proof (minimum)

* memfs can:

  * create/open/read/write files
  * return `stat` metadata including mode/uid/gid/size/times (times can be coarse)
* OFD semantics:

  * cloned `VfsHandle` shares offset
  * `pread/pwrite` do not affect shared offset
  * append is serialized per-OFD
* Permission model:

  * traversal and open checks work in memfs under `PosixPolicy { enforce: true }`
  * checks are invoked from the same VFS path traversal flow (no Wasix duplication)

---

If you want, I can also provide **exact Rust skeletons** for `handle.rs` and `metadata.rs` (module layout + type stubs + TODO markers) that match this spec and are ready for a junior dev to fill in.
