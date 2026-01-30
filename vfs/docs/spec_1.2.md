
Got it – you want an immediate response. I've updated the task accordingly.


Below is a **drop‑in replacement / expansion for Phase 1, Step 1.2** that’s detailed enough to drive implementation and avoid later semantic drift. It’s written to **respect every “Layering contract” rule** and the later‑phase requirements (mounts, overlays, sync+async, FD/OFD correctness, capability truthfulness, confinement, performance).

---

## 1.2 Define core VFS responsibilities

### Step goal

By the end of **1.2**, `vfs-core` must compile and provide a **stable semantic contract + foundational types** that all later phases build on:

* A single place to define **Linux-like VFS semantics boundaries** (what lives in core vs unix vs backends vs wasix).
* A “service object” shape (`Vfs`) and “call context” shape (`VfsContext`) that later phases fill in, so we don’t keep changing public signatures.
* A **byte-oriented POSIX path model** via **`VfsPath` and `VfsPathBuf`** with explicit invariants and fast iteration.
* Stable, semantically meaningful **error kinds** and **flag types** (OS/WASI mappings live elsewhere).
* Hooks for **policy**, **capabilities**, **limits**, and **sync/async** that do not pull in tokio or OS-specific crates.

This step does **not** implement the actual path walker, mount table, overlay logic, hostfs confinement, or Wasix syscalls—those are later steps. But it must lock in *the contracts* so later steps don’t invent competing semantics.

---

## 1.2.1 “What lives in vfs-core” contract (explicit)

### Core is responsible for (semantic source of truth)

These are the invariants that must be documented in `vfs/core/src/lib.rs` module docs and enforced by review:

1. **Namespace + traversal semantics**

   * Single source of truth for mount-aware traversal (`PathWalker` later in Phase 2).
   * Single source of truth for `..` crossing mount boundaries (Linux-like).
   * Single source of truth for symlink resolution rules (follow/no-follow, max depth, trailing slash rules).

2. **OFD semantics**

   * Open File Description state (offset, append/status flags) lives in `vfs-core` handle types.
   * Per-FD flags (CLOEXEC) live *outside* core (Wasix resource table).

3. **Operation contracts**

   * Core defines the *operations Wasix needs* as stable public API signatures (even if stubbed initially).
   * Core defines the internal flag/option types used by those operations (no scattered booleans).

4. **Error and capability model**

   * Core defines stable `VfsErrorKind` and `VfsError` (mapping to `wasi::Errno` happens in `vfs-unix`).
   * Core defines capability vocabulary used to gate ops (actual provider capability flags defined in 1.3, but core must reserve the “gating pattern”).

5. **Security boundary semantics**

   * Core APIs must support true “at”-style operations (base directory handle + relative path).
   * Core must never require callers to do string prefix checks for confinement.

6. **Performance boundaries**

   * Path iteration is allocation-free.
   * IDs and flags are compact and cheap to copy.
   * Hot-path APIs avoid `String`/`PathBuf`/`OsString` in the core contract.

### Core is explicitly **not** responsible for

* OS / WASI translation (`vfs-unix` does errno + open flags mapping).
* Host containment implementation details (`vfs-host` does openat/handles).
* Runtime coupling (tokio/spawn_blocking/block_on lives in `vfs-rt`, not core).
* Implementing actual mount table/path walker/overlay (later phases), **but core defines the stable entry points and types now**.

---

## 1.2.2 Deliverables and file layout

### Deliverables (code)

Create the following modules (small files; no “god file”):

1. `vfs/core/src/lib.rs`

   * Module tree + re-exports
   * Crate-level docs describing responsibilities and layering contract
   * `pub mod` list below

2. `vfs/core/src/error.rs`

   * `VfsErrorKind`, `VfsError`, `VfsResult<T>`

3. `vfs/core/src/path_types.rs`

   * **`VfsPath` + `VfsPathBuf`** (detailed spec in §1.2.4)
   * component iteration and validation helpers

4. `vfs/core/src/ids.rs`

   * `MountId`, `BackendInodeId`, `VfsInodeId`, `VfsHandleId` (newtypes)

5. `vfs/core/src/flags.rs`

   * internal bitflags + option structs (open/lookup/rename/etc.)
   * these are *not* WASI flags; they are VFS semantics flags

6. `vfs/core/src/context.rs`

   * `VfsConfig` (limits like max symlinks/path len)
   * `VfsCred` (uid/gid/groups/umask)
   * `VfsContext` (per-call context: cred + “cwd base” + policy view)

7. `vfs/core/src/policy.rs`

   * `VfsPolicy` trait (default allow policy)
   * future permission hooks are defined here, even if stubbed

8. `vfs/core/src/vfs.rs`

   * `Vfs` struct *shape* and public API *signatures* (can be `todo!()` for now)
   * this is where Wasix will eventually call (Phase 5)
   * includes “at-style” entry points

9. Stub modules that exist but may only contain placeholders + docs (to lock in naming and avoid churn):

   * `vfs/core/src/path.rs` (PathWalker stub)
   * `vfs/core/src/mount.rs` (mount table stub)
   * `vfs/core/src/handle.rs` (OFD handle types stub)
   * `vfs/core/src/inode.rs` (inode semantics stub)
   * `vfs/core/src/node.rs` (FsNode abstraction stub)
   * `vfs/core/src/provider.rs` (reserved for 1.3; in 1.2 it can be an empty module with a comment “defined in step 1.3”)

### Deliverables (tests)

* `vfs/core/tests/path_types.rs` (fast unit tests for `VfsPath`/`VfsPathBuf`)
* `vfs/core/tests/error.rs` (error construction + formatting + kind stability)

### Deliverables (docs)

* Update `vfs/core/README.md` (or add if missing) with:

  * a “what goes where” table (core/unix/host/wasix)
  * explicit statement: “all traversal semantics live in PathWalker in core”
  * the `VfsPath` contract (byte paths, separator, validation)

---

## 1.2.3 Core public API shape (what Wasix will call later)

Even in 1.2, define the *signatures* so later steps don’t force churn.

### Core entry point type

* `pub struct Vfs { inner: Arc<VfsInner> }`
* `VfsInner` will *eventually* contain:

  * provider registry (from step 1.3 / 3.2)
  * mount table snapshot (Phase 3)
  * policy handle (Phase 2.5)
  * limits/hooks (optional later)

### “at”-style operations must be first-class

Core APIs must accept a **base directory reference** *not* a string prefix. In 1.2 we define the base type even if it’s not fully wired:

* `pub enum VfsBaseDir<'a> { Cwd, Handle(&'a VfsDirHandle) }`

  * `Cwd` means “use `VfsContext.cwd`”
  * `Handle` means “resolve relative to this directory handle”
* This keeps us compliant with “at-style behavior is not optional”.

### Minimal op signature set (defined now; implemented later)

Define these as methods on `Vfs` (sync), and later we can add async equivalents or split traits—but **the names and option structs must match later steps**.

Examples (signatures only in 1.2):

* `openat(ctx, base, path, OpenOptions) -> VfsResult<VfsHandle>`
* `statat(ctx, base, path, StatOptions) -> VfsResult<VfsMetadata>`
* `mkdirat(ctx, base, path, MkdirOptions) -> VfsResult<()>`
* `unlinkat(ctx, base, path, UnlinkOptions) -> VfsResult<()>`
* `renameat(ctx, base_old, old_path, base_new, new_path, RenameOptions) -> VfsResult<()>`
* `readlinkat(ctx, base, path, ReadlinkOptions) -> VfsResult<VfsPathBuf>`
* `symlinkat(ctx, base, link_path, target: &VfsPath, SymlinkOptions) -> VfsResult<()>`
* `readdir(ctx, dir_handle, ReadDirOptions) -> VfsResult<DirStreamHandle>` (can be stubbed)

**Important:** this locks in the “semantic contract”: Wasix passes `VfsPath` and option structs; core handles traversal and semantics.

---

## 1.2.4 `VfsPath` and `VfsPathBuf` specification

This is the most important missing piece; it impacts every later phase (path walker, mount table, hostfs translation, object stores, overlays, WASI).

### Design requirements (derived from the full plan)

* **POSIX semantics, Linux-like**: separator `/`, byte paths, no implicit Windows semantics.
* **Non-UTF-8 must be representable**: WASI paths are bytes; don’t force UTF‑8.
* **Allocation-free traversal**: component iteration must not allocate.
* **Preserve caller intent**: keep raw bytes including repeated slashes and trailing slash, because trailing slash affects semantics (`ENOTDIR` vs `ENOENT` cases).
* **Fast validation**: detect illegal NUL bytes once at boundary; don’t repeatedly scan in hot loops.
* **Backend constraints are capability-driven**: core allows arbitrary bytes; hostfs/windows may reject later with a clear error.

### Type definitions

Implement a Path/PathBuf-like pair, but byte-oriented.

#### Option A (recommended): DST-style `&VfsPath` like `std::path::Path`

This is the most ergonomic and performant API surface.

* `pub struct VfsPath { inner: [u8] }` (unsized)
* `pub struct VfsPathBuf { inner: Vec<u8> }`

Key API:

* `impl VfsPath { pub fn new(bytes: &[u8]) -> &VfsPath }`
* `impl VfsPathBuf { pub fn as_path(&self) -> &VfsPath }`

This requires a tiny, auditable unsafe cast in `VfsPath::new` / `VfsPathBuf::as_path`. If you want to keep unsafe quarantined:

* Put the unsafe in `path_types.rs` only
* Add a module-level comment explaining the cast invariants (exactly like `std::path::Path` does internally)
* Add tests to ensure roundtrips preserve bytes

*(If you absolutely want “no unsafe in core”, you can do a lifetime wrapper type instead, but you lose ergonomics and some trait impls. The plan assumes DST-style for developer usability.)*

### Invariants and validation

`VfsPath` and `VfsPathBuf` store **raw bytes** and do **not** canonicalize.

They must provide **two tiers** of API:

1. **Unchecked constructors** (internal use only)

   * `VfsPath::new(bytes)` does not scan for NUL by default (fast)
   * `VfsPathBuf::from_vec_unchecked(vec)` similar

2. **Checked validation helpers** (used at syscall boundary)

   * `fn validate(&self, cfg: &VfsConfig) -> VfsResult<()>`

     * must reject:

       * any `0x00` byte (POSIX NUL terminator issues)
       * length > `cfg.max_path_len` (configurable)
     * must *not* reject:

       * empty path (let ops decide semantics; some syscalls treat empty as `ENOENT`/`EINVAL`)
       * repeated slashes
       * `.` / `..` components (PathWalker handles semantics)

**Why this split:** Wasix should validate once per syscall and then pass `&VfsPath` around without rescanning.

### Separator and absolute/relative rules

* Separator is **ASCII slash** `/` (`0x2F`).
* `VfsPath::is_absolute()` is `bytes.first() == Some(b'/')`.
* Multiple leading slashes:

  * Treat `//` exactly like `/` (Linux-like enough for our use; we do **not** implement POSIX “implementation-defined” `//` special casing).

### Trailing slash and empties (must be observable)

Provide:

* `fn has_trailing_slash(&self) -> bool`

  * true if length > 0 and last byte is `/`
  * note: `/` itself is also “trailing slash”, but PathWalker will special-case root

Provide:

* `fn is_empty(&self) -> bool` (length == 0)

Provide:

* `fn is_root(&self) -> bool` (exactly `[b'/']` or possibly any all-slash sequence? pick one)

  * **Recommendation:** define root as exactly `/` after normalization, but since we’re not normalizing here:

    * `is_root_raw`: bytes are all `/` and length>0
    * `is_root_canonical`: bytes == `/`
  * PathWalker later can treat all-slash as root.

### Component iteration (allocation-free)

Define a component iterator that yields slices of the original path:

* `pub struct VfsComponents<'a> { /* holds &VfsPath and cursor */ }`
* `pub enum VfsComponent<'a> { RootDir, CurDir, ParentDir, Normal(&'a [u8]) }`

Rules:

* Skip empty components caused by repeated slashes (except root).
* For an absolute path:

  * first yielded item may be `RootDir`, then normals.
* For relative path:

  * no `RootDir`.
* Recognize `.` and `..` lexically:

  * `.` yields `CurDir`
  * `..` yields `ParentDir`
* Do **not** resolve them here (PathWalker does).

Also provide a **raw normal component iterator** for fast lookup loops:

* `fn normal_components(&self) -> impl Iterator<Item=&[u8]>`

  * yields only “Normal” segments, skipping `.` and `..`
  * used later in object store backends and overlay directory merging

### Component validation type for backend lookup

Backends typically want *a single name* that must not contain `/` or NUL. Add a helper:

* `pub struct VfsName<'a>(&'a [u8]);`
* `impl<'a> VfsName<'a> { fn new(bytes: &'a [u8]) -> VfsResult<Self> }`

  * rejects:

    * empty
    * contains `/`
    * contains NUL

And an owned form if needed:

* `pub struct VfsNameBuf(Vec<u8>)`

This prevents every backend from re-implementing “name validity”.

### Joining and pushing (owned path manipulation)

`VfsPathBuf` must support efficient building without introducing semantic ambiguity:

* `fn push(&mut self, seg: &VfsPath)` (path-join semantics)

  * If `seg.is_absolute()`: replace self entirely (like PathBuf)
  * Else:

    * ensure exactly one slash between existing and new segment:

      * if self empty or self ends with `/` → append seg bytes (minus leading slashes?)
      * else → append `/` then seg bytes (minus leading slashes?)
  * **Be explicit in docs** about whether you strip leading slashes from `seg` when joining; recommendation:

    * for relative join, strip any leading slashes in seg to avoid “silent absolute injection”
    * but expose an explicit `push_raw()` if you need exact raw concatenation

* `fn push_name(&mut self, name: VfsName<'_>)`

  * always appends exactly one separator and the raw name bytes

* `fn pop(&mut self) -> bool`

  * remove last normal component (lexical, not resolving symlinks)
  * used mainly for internal construction (debug paths, synthetic paths)

* `fn set_extension_bytes(&mut self, ext: &[u8]) -> bool` (optional; only if clearly needed)

### Display/Debug behavior (don’t assume UTF-8)

* `Debug` should print byte-escaped form (e.g. `b"/tmp/\xFF"` style) so logs are not lossy.
* Avoid implementing `Display` by default; or implement `Display` as **lossy UTF-8** behind a feature flag (to prevent accidental corruption/assumptions).

### Interop conversions

Core should define conversion helpers without pulling in OS-specific crates:

* `impl From<Vec<u8>> for VfsPathBuf`
* `impl From<&[u8]> for &VfsPath` via `VfsPath::new`
* `fn to_vec(&self) -> Vec<u8>` (owned copy)
* `fn to_utf8(&self) -> Option<&str>` (only if valid utf8; no allocation)
* `fn to_utf8_lossy(&self) -> Cow<str>` (allocates only if needed; might be in a debug module)

### Error behavior for invalid paths

Validation should return:

* `VfsErrorKind::InvalidInput` for NUL bytes, too-long paths, invalid component usage (if checked)
* Avoid returning `NotFound` for invalid inputs; keep error kinds semantically correct for errno mapping later.

---

## 1.2.5 Core error model (must support later errno mapping cleanly)

In `error.rs` define:

* `pub type VfsResult<T> = Result<T, VfsError>;`

* `#[non_exhaustive] pub enum VfsErrorKind {`

  * `NotFound`
  * `NotDir`
  * `IsDir`
  * `AlreadyExists`
  * `PermissionDenied`
  * `InvalidInput`
  * `TooManySymlinks`
  * `NotSupported`
  * `CrossDevice`
  * `Busy`
  * `DirNotEmpty`
  * `ReadOnlyFs`
  * `WouldBlock`
  * `Interrupted`
  * `Io` (generic fallback)
  * `…` (reserve space; non_exhaustive)
    `}`

* `pub struct VfsError { kind: VfsErrorKind, context: &'static str, source: Option<Box<dyn Error + Send + Sync>> }`

  * `context` is a short static string like `"openat"`, `"lookup"`, etc (cheap)
  * `source` is optional and for debugging only (never relied upon for semantics)

Provide constructors:

* `VfsError::new(kind, context)`
* `VfsError::with_source(kind, context, source)`
* `impl From<std::io::Error> for VfsError` should map to `Io` **without** guessing semantics (guessing happens in vfs-host/vfs-unix).

**Why now:** later phases (vfs-unix errno mapping, hostfs translation, object store) depend on stable kinds.

---

## 1.2.6 Core option/flag types (internal semantics, not WASI)

In `flags.rs`, define internal bitflags and option structs used by the `Vfs` methods.

Examples:

* `bitflags! { pub struct OpenFlags: u32 {`

  * `READ`, `WRITE`, `APPEND`, `TRUNC`, `CREATE`, `EXCL`
  * `DIRECTORY`, `NOFOLLOW`, `CLOEXEC` **(note: CLOEXEC is per-FD; keep it here only if you explicitly document it gets stripped by Wasix)**
  * `SYNC`, `DSYNC` (optional)
  * `NONBLOCK` (status flag; may be partially enforced)
    `}}`

* `pub struct OpenOptions { flags: OpenFlags, mode: Option<u32> /*posix mode bits*/ , resolve: ResolveFlags }`

* `bitflags! { pub struct ResolveFlags: u32 {`

  * `NO_SYMLINK_FOLLOW` (covers AT_SYMLINK_NOFOLLOW)
  * `BENEATH` / `IN_ROOT` (future: confinement semantics, similar to openat2 RESOLVE_BENEATH)
  * `NO_MAGICLINKS` (optional)
    `}}`

* `pub struct StatOptions { resolve: ResolveFlags, follow: bool, require_dir_if_trailing_slash: bool }`

  * yes, make trailing slash requirement explicit—later tests depend on it

* `pub struct RenameOptions { flags: RenameFlags }`

* `bitflags! { pub struct RenameFlags: u32 { NOREPLACE, EXCHANGE /*maybe later*/ } }`

**Why now:** these option structs prevent later API drift into “15 booleans”.

---

## 1.2.7 Call context and policy hooks (for permissions + future jail semantics)

In `context.rs` define:

* `pub struct VfsConfig {`

  * `max_symlinks: u8` (or `u16`)
  * `max_path_len: usize`
  * `max_name_len: usize`
  * `…` (reserve)
    `}`

* `pub struct VfsCred { uid: u32, gid: u32, groups: smallvec::SmallVec<[u32; 8]>, umask: u32 }`

  * groups must be cheap to clone; prefer smallvec

* `pub struct VfsContext {`

  * `cred: VfsCred`
  * `cwd: VfsInodeId` (or a directory handle id; decide now)
  * `config: Arc<VfsConfig>` (or borrowed)
  * `policy: Arc<dyn VfsPolicy>`
    `}`

**Important decision for later “AT_FDCWD”:**

* Core must be able to resolve relative paths against a “current working directory”.
* Since Wasix should not implement path hacks, Wasix must supply the cwd handle/inode into `VfsContext`.
* Recommendation: store `cwd` as a **directory handle** (or inode id + mount) so it stays valid even if the textual cwd string changes.

In `policy.rs` define:

* `pub trait VfsPolicy: Send + Sync {`

  * `fn check_path_op(&self, ctx: &VfsContext, op: VfsOp, target: &VfsInodeId, meta: Option<&VfsMetadata>) -> VfsResult<()>;`
  * `fn check_open(&self, ctx: &VfsContext, target: &VfsInodeId, opts: &OpenOptions, meta: Option<&VfsMetadata>) -> VfsResult<()>;`
  * default implementation: allow all
    `}`

Where `VfsOp` is a small enum: `Read`, `Write`, `Exec`, `Create`, `Delete`, `Metadata`, etc.

**Why now:** Phase 2.5 needs policy hooks for POSIX perms and jail behavior; defining the trait early prevents later invasive signature changes.

---

## 1.2.8 Concurrency and ownership rules (declared now)

Document in `vfs.rs` + crate docs:

* `Vfs` is cheap clone (`Arc`).
* Later mount table will be snapshot-based (ArcSwap) to keep reads lock-light (Phase 3), but **1.2 must already commit to “read-mostly” design**.
* Backends are `Send + Sync` and can be stored behind `Arc<dyn …>` (defined later).
* `VfsPath` is immutable view; `VfsPathBuf` is owned and mutable.

No code required yet beyond struct placeholders, but the docs must state the intended model so Phase 3 doesn’t surprise Phase 2.

---

## 1.2.9 Acceptance criteria for Step 1.2

### Build checks

* `cargo check -p vfs-core` succeeds.
* `cargo test -p vfs-core` succeeds.

### Contract checks (cheap, explicit)

Add a small test or doc-check that:

* `VfsPath` can represent non-UTF-8 bytes and round-trips without loss.
* `VfsPath::components()` does not allocate and yields expected components for:

  * `b""`
  * `b"/"`
  * `b"//a///b/"`
  * `b"./a/../b"`
* `VfsPath::validate()` rejects NUL bytes and too-long paths.

### Dependency sanity

* `vfs-core` has **no tokio dependency** (verify via `cargo tree -p vfs-core` during review).

---

## 1.2.10 “Update the plan” note (what to paste back)

To update your master plan, replace the old 1.2 section with everything from **“1.2 Define core VFS responsibilities”** through **1.2.10** above, and in the top-level Phase 1 acceptance criteria add:

* “`VfsPath`/`VfsPathBuf` exist, are byte-based, validated, and have allocation-free component iteration with tests.”

---

If you want, I can also draft the **exact module-level doc text** (the “Layering contract” reminder, the `VfsPath` invariants, and the “at-style is not optional” warning) so it can be pasted directly into `vfs/core/src/lib.rs` and `vfs/core/src/path_types.rs`.
