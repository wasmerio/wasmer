# Spec 6.1 — Host filesystem provider (`vfs-host`)

## Status

- **Target plan section**: `fs-refactor.md` → **Phase 6.1 Host filesystem provider**
- **This spec assumes**:
  - **Phase 4 is complete** (sync/async traits + adapters are in-tree).
  - **Phase 5 (Wasix integration) is explicitly delayed until after Phase 6**.
  - We **must not change the existing `vfs-core` public API surface** (traits, structs, type names).

## Summary (one paragraph)

Implement a new `vfs-host` backend that exposes a confined view of the host filesystem through the existing `vfs-core` backend traits (`FsProviderSync` / `FsSync` / `FsNodeSync` / `FsHandleSync`). The provider must implement **security-critical confinement** using **capability handles** (dirfd / handle-based traversal), not string-prefix checks. The implementation must be cross-platform (Linux/macOS/Windows) with well-documented semantic differences and best-effort fallbacks where OS primitives are missing. Async usage is provided via the already-implemented Phase 4 adapters (`vfs/core/src/provider.rs`), so `vfs-host` can implement **sync-only** initially while remaining usable from async contexts.

## Current implementation baseline (what exists today)

This spec is written to match the current repository state:

- **Provider traits and mount configuration** live in:
  - `vfs/core/src/traits_sync.rs` (`FsProviderSync`, `FsSync`, `FsNodeSync`, `FsHandleSync`)
  - `vfs/core/src/traits_async.rs` (`FsProviderAsync`, etc.)
  - `vfs/core/src/provider.rs` (capabilities, `MountRequest`, `ProviderConfig`, and Phase 4 sync/async adapters)
- **Mount table** is implemented in `vfs/core/src/mount.rs` and stores *both* sync and async `Fs` instances; adapters are expected to be used at mount time.
- **Path traversal** is implemented in `vfs/core/src/path_walker.rs` (sync) and has an async counterpart (not shown here) used by Phase 4 tests.
- The **public `Vfs` service object** exists but is currently mostly `NotImplemented` in `vfs/core/src/vfs.rs`. This spec therefore focuses on implementing the backend provider + correctness/security tests that can be written **without** needing `Vfs::openat()` to be completed.
- `vfs/host/src/lib.rs` is currently a placeholder.

## Non-goals (explicitly out of scope for 6.1)

These are deferred either because Phase 5 is moved after Phase 6, or because the current `vfs-core` surface does not yet expose the necessary hooks:

- **No Wasix syscall integration** (Phase 5). In particular:
  - no Wasix FD/resource-table integration
  - no `wasi::Errno` mapping in `vfs-host` (that stays in `vfs-unix` and Phase 5 shims)
  - no rights/flags translation logic owned by Wasix
- **No VFS watch API implementation**, because `vfs-core` currently does not define a watch trait/module. The provider may keep internal scaffolding, but must not claim `FsProviderCapabilities::WATCH` yet.
- **No overlay/union integration** (Phase 6.5).
- **No attempt to fully replicate every POSIX corner case**. Phase 7 will drive compliance and extend both `vfs-core` and providers as needed.

## Goals and acceptance criteria

### Core goals

- Implement `vfs-host` as a **real provider**:
  - `HostFsProvider: FsProviderSync`
  - `HostFs: FsSync`
  - `HostNode: FsNodeSync`
  - `HostHandle: FsHandleSync`
- Provider must mount a host directory as a VFS filesystem instance with **confinement**.
- Must be usable from async code via Phase 4 adapters:
  - `AsyncProviderFromSync` / `AsyncFsFromSync` / `AsyncNodeFromSync` / `AsyncHandleFromSync` (already in `vfs/core/src/provider.rs`)
- Must implement at least:
  - directory traversal: `lookup`, `read_dir`
  - file lifecycle: `create_file`, `open`, `unlink`, `rmdir`, `mkdir`
  - metadata: `metadata`, `set_metadata` (best-effort; see below)
  - rename: `rename`
  - symlinks: `symlink`, `readlink` (platform-dependent; see below)
  - hardlinks: `link` (platform-dependent; see below)

### Security acceptance criteria (must-have)

Within the confines of the current `vfs-core` interface:

- **No string-prefix confinement**: do not implement “`path.starts_with(root)`”-style checks.
- **Confinement must be enforced by capability handles**:
  - On Unix: all operations are performed via `*at()` syscalls using dirfds and validated `VfsName` components.
  - On Windows: all operations are performed relative to a root directory handle using handle-relative APIs (or best-effort emulation when truly missing).
- **Symlink traversal must not allow escaping the mount root via `..` tricks** when the VFS layer requests confinement (`ResolveFlags::BENEATH` / `ResolveFlags::IN_ROOT`).
  - Note: the *policy* for whether confinement is requested is owned by callers (eventually Wasix/`Vfs`), but the host provider must be safe when used with the VFS path walker’s confinement flags.
- **Race resistance**: component-by-component traversal must be robust against TOCTOU escapes as much as the OS allows, by ensuring traversal is driven by opened directory capabilities (see “Confinement model”).

### Functional acceptance criteria (tests)

Add backend tests in `vfs/host/tests/` that demonstrate:

- Register + mount the host provider via `vfs_core::FsProviderRegistry`.
- Basic CRUD:
  - create file, write bytes via `VfsHandle` (from `vfs-core`) and read back
  - create dir, readdir includes it
  - rename within the mount
  - unlink/rmdir behavior and error kinds
- Symlink behavior on Unix:
  - create a symlink and `readlink()` returns the correct target bytes
- Confinement behavior using `vfs_core::PathWalker`:
  - construct a mount table with hostfs as the root mount
  - build a `VfsContext`
  - ensure `WalkFlags { resolve_beneath: true }` (or `in_root: true`) prevents escaping even with malicious symlink targets

## Deliverables (files and modules)

### Required files

- `vfs/host/src/lib.rs` — crate entry; exports provider/config types.

### Recommended internal module layout

`vfs/host/src/`:

- `lib.rs`
- `config.rs` — `HostFsConfig` and validation helpers
- `provider.rs` — `HostFsProvider` implementation
- `fs.rs` — `HostFs` (`FsSync`) and shared state
- `node.rs` — `HostNode` (`FsNodeSync`)
- `handle.rs` — `HostHandle` (`FsHandleSync`)
- `platform/`:
  - `platform/mod.rs`
  - `platform/unix.rs`
  - `platform/windows.rs`
  - `platform/macos.rs` (optional; macOS is Unix-y but often needs special casing)

## Public API of `vfs-host` (crate exports)

The crate should export a minimal stable surface:

```rust
pub struct HostFsProvider;

#[derive(Clone, Debug)]
pub struct HostFsConfig {
    /// Host path of the directory to expose as the filesystem root.
    pub root: std::path::PathBuf,

    /// Optional: enable a stricter validation mode that rejects non-directory roots, etc.
    pub strict: bool,
}
```

Notes:

- `HostFsConfig` must be usable as `&dyn vfs_core::provider::ProviderConfig` (it automatically is, due to the blanket impl in `vfs-core`).
- The provider name must be **exactly** `"host"` to match the plan and registry normalization rules.

## Dependency and feature plan

### Crate dependencies (expected)

In `vfs/host/Cargo.toml`:

- `vfs-core = { path = "../core" }`
- `cfg-if`
- `tracing` (optional but recommended)
- Platform deps:
  - Unix: `libc` **or** `rustix`
  - Windows: `windows-sys` (or `windows` crate) if std APIs are insufficient

### Do not pull in Wasix types

`vfs-host` must remain usable in non-Wasix builds. If it needs Unix helpers from `vfs-unix` (e.g. error normalization), depend on `vfs-unix` with `default-features = false` to avoid pulling the WASI mapping feature:

```toml
vfs-unix = { path = "../unix", default-features = false, features = ["host-errno"] }
```

This is optional; `vfs-host` may also implement its own `std::io::Error` → `VfsErrorKind` mapping, but it must remain consistent with `vfs-unix` over time.

## Confinement model (security-critical design)

### Why capability handles are required

The VFS core is byte-path based (`VfsPath`, `VfsName`) and path resolution is implemented in `vfs-core` (`PathWalker`). A host backend must **not** rely on string path checks to enforce confinement because those checks are racy and bypassable under rename/symlink attacks.

Instead, the backend must model the filesystem using **capabilities**:

- **Directory nodes** must be backed by a **directory handle** (Unix: dirfd; Windows: directory HANDLE).
- All child operations (lookup/create/remove/rename) must be performed using `*at()`-style syscalls relative to that handle.

This matches the plan requirement (“enforce confinement using capability handles (dirfd/openat-style), not string prefix checks”).

### Representing nodes without leaking global paths

The key is: **`FsNodeSync::lookup` takes a validated `VfsName`, not a path**. This makes it possible to implement race-resistant traversal:

- For a directory node `D`, `lookup("child")` does:
  - `openat(D.dirfd, "child", O_PATH | O_NOFOLLOW | ...)` (Unix)
  - or a handle-relative open without following reparse points (Windows)
- The returned node carries:
  - a stable backend inode id
  - its file type
  - if it is a directory: its own directory handle for subsequent lookups
  - enough locator info to perform operations that require the parent dir + name (for operations like `unlinkat`, `renameat`), without reconstructing absolute paths

### Required internal node identity fields

Every `HostNode` must carry:

- **A reference to the mount root state** (`Arc<HostFsInner>`) for shared configuration and consistent inode/id computation.
- **A “locator”** for “parent directory + entry name”, so we can perform `unlinkat`, `renameat`, etc. using `*at` syscalls safely.

Suggested internal representation:

```rust
struct Locator {
    // Root has no parent locator.
    parent: Option<Arc<HostDir>>,
    name: Option<vfs_core::VfsNameBuf>,
}

struct HostDir {
    // Platform directory capability handle
    dir: platform::DirHandle,
    inode: vfs_core::BackendInodeId,
    locator: Locator,
}

enum HostNodeKind {
    Dir(Arc<HostDir>),
    File { inode: BackendInodeId, locator: Locator },
    Symlink { inode: BackendInodeId, locator: Locator },
    // (Optional future: special files)
}
```

Important:

- `locator.parent` is an `Arc` to keep the parent directory capability alive as long as any child nodes exist (prevents UAF / stale dirfd usage).
- This forms a *tree of capabilities* rooted at the mount root directory handle.
- The VFS path walker’s “parent stack” uses already-opened directories; `..` traversal does not require evaluating `..` on the host, which is how confinement survives `..` tricks.

### What “confinement” means in Phase 6.1

Given current `vfs-core`:

- The **hostfs backend** must ensure that all operations are performed relative to already-opened directory capabilities, and **never** by concatenating a global host path string.
- The **VFS layer** decides when to enforce confinement via `ResolveFlags` (e.g. `BENEATH` / `IN_ROOT`) and ultimately via `WalkFlags` in the path walker.
- When confinement is requested, **symlink targets containing `..` or absolute paths must not be able to escape the mount root**:
  - With capability-based traversal, “escape” requires the path walker to pop above the root anchor; `WalkFlags` already defines this behavior.
  - The host provider must not introduce escape hatches by resolving via host-global paths.

## Platform-specific implementation details

### Unix (Linux + macOS)

#### Directory handle type

Use an owned file descriptor type (`OwnedFd` or `std::fs::File`) opened as a directory:

- For directory capabilities, prefer `O_RDONLY | O_DIRECTORY | O_CLOEXEC` (portable)
- On Linux, if available, use `O_PATH | O_DIRECTORY | O_CLOEXEC` to avoid permission pitfalls and to model capabilities directly.

#### Opening without following symlinks

For `lookup` and symlink-aware operations:

- Use `openat(dirfd, name, O_NOFOLLOW | ...)` (or `openat2` with `RESOLVE_NO_SYMLINKS` for the final component where supported).
- Determine file type via `fstat` or `fstatat(AT_SYMLINK_NOFOLLOW)`.

#### Metadata

Implement `FsNodeSync::metadata` using `fstatat(AT_SYMLINK_NOFOLLOW)` against the locator’s parent dirfd + name, and for root using `fstat` on the root dirfd.

Fill `vfs_core::VfsMetadata`:

- `file_type`: Directory / RegularFile / Symlink
- `mode`: from `st_mode & 0o7777` mapped into `VfsFileMode`
- `uid`, `gid`: from `st_uid`, `st_gid`
- `size`: from `st_size` (0 for directories is acceptable; use real size if available)
- timestamps: map `st_atime`, `st_mtime`, `st_ctime` to `VfsTimespec`
- `rdev_major/minor`: best-effort for device nodes (optional; can be 0 in 6.1)

#### Inode identity (BackendInodeId)

Backend inode identity must be stable per mount:

- Prefer \(dev, ino\) on Unix:
  - `st_dev` + `st_ino` hashed to a non-zero `u64`
  - If `st_ino` is non-zero and the FS guarantees per-device uniqueness, you may pack it directly (but hashing is safer).
- Ensure the computed `u64` is never 0; if hashing yields 0, map it to 1 (or rehash with a fixed tweak).

Expose capability bits accordingly:

- Provider caps: `UNIX_PERMISSIONS`, `UTIMENS`, `SYMLINK`, `HARDLINK` (where supported)
- FS caps: `VfsCapabilities::SYMLINKS`, `HARDLINKS`, `CHMOD`, `CHOWN`, `UTIMENS` as applicable

#### File operations

`FsHandleSync` should be implemented using `std::fs::File` and `std::os::unix::fs::FileExt`:

- `read_at(offset, buf)` → `FileExt::read_at`
- `write_at(offset, buf)` → `FileExt::write_at`
- `fsync()` → `File::sync_all`
- `flush()` → `File::sync_data` or no-op (prefer `sync_data` if available)
- `set_len()` → `File::set_len`
- `get_metadata()` / `len()` → `File::metadata`
- `dup()` → `File::try_clone` (returns `Some` on success)

`OpenFlags::APPEND` is handled at the VFS handle layer (OFD status flags in `vfs-core/src/handle.rs`), so the backend handle can ignore it (or optionally open with `O_APPEND` as an optimization).

#### `rename` behavior

`FsNodeSync::rename` takes `new_parent: &dyn FsNodeSync`:

- If `new_parent` is not a `HostNode` from the **same mount**, return `VfsErrorKind::CrossDevice`.
- Implement rename using:
  - Linux: `renameat2` for `noreplace`/`exchange` where available
  - Otherwise: `renameat` with best-effort emulation:
    - `exchange` → `NotSupported`
    - `noreplace` → pre-check + rename (racy; document)

### Windows

Windows does not have a direct `openat` model, and path bytes are not arbitrary; we must define explicit behavior:

- **Name encoding**:
  - Treat `VfsName`/`VfsPath` bytes as UTF-8 on Windows.
  - If bytes are not valid UTF-8, return `VfsErrorKind::InvalidInput`.
- **Confinement**:
  - Open the mount root as a directory handle.
  - Resolve child names relative to that handle (e.g. using handle-based APIs, or by maintaining a chain of directory handles).
  - Avoid string prefix checks. If certain APIs require paths, use absolute paths only as a last-resort fallback and document that it is not race-free.
- **Symlinks/reparse points**:
  - Prefer opening without following reparse points (similar to Unix `O_NOFOLLOW`), and expose symlinks as `VfsFileType::Symlink` where possible.
  - If robust symlink support is not feasible in the initial pass, return `NotSupported` for `symlink`/`readlink` and do not claim provider SYMLINK capability on Windows.

### macOS

macOS is Unix-like but lacks `openat2`.

- Use `openat`, `fstatat`, `readlinkat`, etc. and enforce capability traversal.
- Document limitations vs Linux (no `renameat2`, reduced race-hardening).

## Detailed mapping to `vfs-core` traits

This section defines the **exact** trait methods `vfs-host` must implement and the required semantics.

### `HostFsProvider` (`FsProviderSync`)

Implement:

- `fn name(&self) -> &'static str` → `"host"`
- `fn capabilities(&self) -> FsProviderCapabilities`
  - Must reflect platform support:
    - Unix: `SYMLINK`, `HARDLINK`, `UNIX_PERMISSIONS`, `UTIMENS`, `SEEK`
    - Windows: likely `SEEK`, maybe `CASE_PRESERVING` (best-effort); omit symlink/hardlink unless implemented
  - Must **not** include `WATCH` yet.
- `fn validate_config(&self, config: &dyn ProviderConfig) -> VfsResult<()>`
  - Must downcast to `HostFsConfig` and reject other config types with `InvalidInput`.
  - Must validate that `root` exists and is a directory (or if `strict=false`, allow creating it later; pick one and document).
- `fn mount(&self, req: MountRequest<'_>) -> VfsResult<Arc<dyn FsSync>>`
  - Must enforce mount flags:
    - If `req.flags` includes `READ_ONLY`, operations that mutate must return `ReadOnlyFs`.
  - Must open the root directory handle and return `HostFs`.

### `HostFs` (`FsSync`)

Implement:

- `provider_name()` → `"host"`
- `capabilities()` → `VfsCapabilities` that match what this instance can do (platform + mount flags)
- `root()` → a `HostNode` representing the mount root directory
- `node_by_inode()` (optional) → `None` in 6.1 (can be added later if we maintain an inode→node cache)

### `HostNode` (`FsNodeSync`)

Implement the following methods with the semantics described.

#### `inode()` and `file_type()`

- `inode()` must return a stable `BackendInodeId` for the lifetime of the mount.
- `file_type()` must reflect the node’s identity **without following symlinks**:
  - `Directory`, `RegularFile`, `Symlink`

#### `metadata()`

Must return a complete `VfsMetadata` best-effort:

- If the platform cannot supply fields (e.g. uid/gid on Windows), fill safe defaults (`0`) and document it.
- **Mount id limitation (current `vfs-core` reality)**: `VfsMetadata.inode` is a `VfsInodeId { mount, backend }`, but backends do not currently receive the real `MountId`. Existing code (e.g. `vfs-mem`) uses `MountId::from_index(0)` as a placeholder. For Phase 6.1, `vfs-host` should follow that convention and document it as a known limitation to be tightened once mount-aware stat plumbing is finalized in `vfs-core`.

#### `set_metadata(set: VfsSetMetadata)`

Implement the following fields:

- **`size`**: if set and node is a regular file, truncate/extend to size; else:
  - directories → `InvalidInput` (match memfs behavior)
  - symlinks → `NotSupported` (best-effort)
- **`mode`**: Unix `chmodat` best-effort; Windows `NotSupported`
- **`uid`/`gid`**: Unix `chownat` best-effort; Windows `NotSupported`
- **`atime`/`mtime`**: Unix `utimensat` best-effort; Windows best-effort if implemented
- **`ctime`**: generally `NotSupported` on all platforms (ctime is kernel-maintained)

If mount flags include `MountFlags::READ_ONLY`, return `ReadOnlyFs` for any mutation.

#### `lookup(name: &VfsName)`

Preconditions:

- The current node must be a directory, else `NotDir`.
- Must not follow symlinks for the final component; the returned node must be classified as `Symlink` if it is one.

Implementation outline (Unix):

- Use `openat(dirfd, name, O_NOFOLLOW | O_CLOEXEC | O_PATH?)` to acquire a capability to the child.
- Determine the child’s type using `fstat`/`fstatat(AT_SYMLINK_NOFOLLOW)`.
- If the child is a directory, open a directory handle suitable for subsequent `openat` operations.

If the child does not exist: `NotFound`.

#### `create_file(name, opts: CreateFile)`

Semantics (match memfs contract in `vfs/core/src/node.rs`):

- If entry exists:
  - If it is a directory → `IsDir`
  - If `opts.exclusive` → `AlreadyExists`
  - If `opts.truncate` → truncate existing file to 0
  - Return the existing file node
- If entry does not exist:
  - Create it, apply mode if provided (Unix only), return file node

Implementation notes:

- Use `openat` with `O_CREAT` and optionally `O_EXCL`, `O_TRUNC`.
- Ensure you do not accidentally follow symlinks when creating/truncating.

#### `mkdir(name, opts: MkdirOptions)`

- Create a directory.
- If exists: `AlreadyExists`.
- Apply mode if supported.
- Return the new directory node.

#### `unlink(name, opts: UnlinkOptions)` and `rmdir(name)`

- `unlink` removes a non-directory entry by default.
  - If `opts.must_be_dir == true`, it must behave like `rmdir` or error.
- `rmdir` removes a directory and must return:
  - `DirNotEmpty` when not empty
  - `NotDir` if target is not a directory

Implementation notes (Unix):

- Prefer `unlinkat(dirfd, name, 0)` for `unlink`
- Prefer `unlinkat(dirfd, name, AT_REMOVEDIR)` for `rmdir`

#### `read_dir(cursor, max) -> ReadDirBatch`

The `vfs-core` cursor model is an opaque cookie (`VfsDirCookie(u64)`).

For 6.1 implement a simple, deterministic cursor model:

- Read the entire directory listing, **sort by name bytes** (determinism for tests).
- Interpret `cursor` as an index into the sorted list.
- Return up to `max` entries and set `next` to `Some(cookie)` if more remain.

This matches the cookie model used by `vfs-unix` dirent encoding (`vfs/unix/src/dirents.rs`).

#### `rename(old_name, new_parent, new_name, opts: RenameOptions)`

Rules:

- If `new_parent` is not a `HostNode` from the same mount → `CrossDevice`.
- If `opts.exchange` is requested and platform cannot support it → `NotSupported`.
- If `opts.noreplace` is requested:
  - Linux: use `renameat2(..., RENAME_NOREPLACE)` where available
  - else: best-effort pre-check (racy), or return `NotSupported` if we want strict correctness

#### `link(existing, new_name)` (hardlink)

- If hardlinks are not supported on the platform/filesystem, return `NotSupported`.
- If `existing` is not within the same mount, return `CrossDevice`.

Unix: implement via `linkat`.

#### `symlink(new_name, target)` and `readlink()`

- On Unix:
  - `symlink` should create a symlink with raw target bytes.
  - `readlink` returns the raw target bytes as `VfsPathBuf`.
- On Windows:
  - Either implement reparse-point aware behavior or return `NotSupported` and do not advertise symlink capability.

### `HostHandle` (`FsHandleSync`)

The handle trait is an **offset-based** I/O API (`read_at`/`write_at`) that the VFS layer uses to implement OFD semantics (`vfs/core/src/handle.rs`).

Implement:

- `read_at(offset, buf)`:
  - perform a positioned read without changing any shared offset
  - return 0 on EOF
- `write_at(offset, buf)`:
  - positioned write
- `flush()`:
  - best-effort; may be a no-op on some platforms
- `fsync()`:
  - best-effort durability
- `get_metadata()`:
  - return `VfsMetadata` for the opened file (best-effort)
- `set_len(len)` and `len()`:
  - implement where supported
- `dup()`:
  - return `Some(cloned_handle)` if the platform supports handle duplication (Unix `try_clone`)
- `is_seekable()`:
  - `true` for regular files, `false` for special handles if any are supported later

## Integration with `vfs-core` mount table and adapters (Phase 4 reality)

Because Phase 4 is complete:

- `vfs-host` can implement **only** `FsProviderSync`.
- Async callers should mount it via `vfs_core::provider::AsyncProviderFromSync` using a `VfsRuntime`.

For tests and examples, use `vfs_rt::InlineTestRuntime` and the existing adapter types.

## Testing plan (what to implement in `vfs/host/tests/`)

### 1) Provider registry mount smoke test

- Create `FsProviderRegistry`.
- Register `HostFsProvider`.
- Mount with a temp directory root.
- Assert `fs.provider_name() == "host"` and `fs.root()` is a directory.

### 2) File CRUD

- `root.create_file("file", opts)` then `node.open(OpenOptions { READ|WRITE })`
- Write data via `FsHandleSync::write_at`, read back via `read_at`
- Validate `metadata.size` matches.

### 3) Directory operations

- `root.mkdir("dir")`
- `root.read_dir(None, max)` includes `dir`

### 4) Rename semantics

- Create `a`, rename to `b`, ensure `lookup("a")` fails and `lookup("b")` succeeds.

### 5) Symlink semantics (Unix-only)

- `root.symlink("link", "target")`
- `root.lookup("link")` returns `Symlink`
- `readlink()` returns `"target"`

### 6) Confinement behavior using `PathWalker`

Even though `vfs/core/src/vfs.rs` is not implemented yet, we can test confinement at the path-walker level:

- Create a hostfs root backed by a temp dir.
- Create `inside/` and a symlink `inside/escape -> ../..` (or absolute).
- Build a `MountTable` with hostfs as the root mount.
- Construct a `VfsContext` with `cwd` at `inside/`.
- Use `PathWalker::resolve` with `WalkFlags { resolve_beneath: true }` (or `in_root: true`) and verify it refuses to escape.

## Documentation requirements

The implementation must document:

- Platform differences (Windows encoding, symlink semantics, case-sensitivity/preservation).
- Which provider capabilities are claimed on which platform.
- Which `VfsCapabilities` are exposed from `FsSync::capabilities()`.
- Any best-effort/racy fallbacks (e.g. `RENAME_NOREPLACE` on platforms without `renameat2`).

## Follow-ups (not required for 6.1, but should be noted)

- Implement `vfs-host` as an async-native provider (optional; Phase 4 adapters already make it usable).
- Add watch API once `vfs-core` defines it; then expose inotify/FSEvents/ReadDirectoryChangesW and claim `FsProviderCapabilities::WATCH`.
- Add inode-to-node caching and implement `FsSync::node_by_inode()` for `STABLE_INODES` backends.
- Tighten mount-aware `VfsMetadata.inode.mount` filling once `vfs-core` fully owns mount-aware stat plumbing.


