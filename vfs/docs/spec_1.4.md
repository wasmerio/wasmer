
## Step 1.4 Spec: `vfs/unix` POSIX/WASI Translation Crate

### Why this crate exists (and how it relates to the rest of the plan)

`vfs/unix` is the **single source of truth** for “translation glue” between:

* **VFS semantic layer** (`vfs/core`: Linux-like traversal + inode/handle semantics + errors), and
* **callers and OS/WASI surfaces** (primarily `lib/wasix` syscalls; secondarily `vfs/host` platform interop)

This crate **must not implement filesystem semantics** (no path walking, no mount logic, no permission model). It only provides:

1. **Error mapping**: `vfs/core::VfsError` → `wasi::Errno` (and optionally → host errno / `std::io::ErrorKind` for diagnostics).
2. **Flag translation**: WASI/WASIX “open-like” flags → `vfs/core` internal flags (and reverse where needed).
3. **Directory entry encoding**: a `getdents`-style encoder used by Wasix readdir/getdents APIs.

This crate is “unix” mostly by historical naming: it’s where POSIX-y details live, but it must remain usable on all platforms (Linux/macOS/Windows) because Wasix runs everywhere. Any truly Unix-only code is behind `cfg(unix)`.

---

## Goals and Non-goals

### Goals

* **One mapping table** from `VfsErrorKind` to `wasi::Errno` used by *all* Wasix syscall shims (Phase 5).
* **One translation path** from WASI openat-like flags into a VFS-internal `OpenOptions` type used by `vfs/core` dispatch + backends.
* A **reusable dirent encoder** with deterministic output and clear alignment rules.
* Keep `vfs/core` platform-agnostic and free of `unsafe` OS calls, while still enabling `vfs/host` to use clean helpers.

### Non-goals

* No mount/path semantics (that stays in `vfs/core::PathWalker` per layering contract).
* No WASI rights gating (Wasix does that first per the plan).
* No backend capability logic (that’s `FsProviderCapabilities` in `vfs/core`).
* No runtime coupling (tokio etc. stays out; async bridging stays in `vfs/rt`).

---

## Crate layout and module structure

`vfs/unix/src/lib.rs` should be tiny and just re-export modules.

Recommended module split (no huge files):

```
vfs/unix/src/
  lib.rs
  errno.rs
  open_flags.rs
  dirents.rs
  filetype.rs
  io_error.rs        (optional, but useful for vfs/host)
  tests/
    errno_mapping.rs
    open_flags.rs
    dirents.rs
```

### Dependencies (Cargo.toml)

* Required:

  * `vfs-core = { path = "../core" }`
  * The same WASI types crate used by `lib/wasix` (whatever it is in-repo). The plan references `wasi::Errno`; use that exact crate/path to avoid duplicate enums.
* Optional (feature-gated):

  * `rustix` (or `libc`) behind `cfg(unix)` for errno constants if needed.
  * `windows-sys` behind `cfg(windows)` only if you decide to map Win32 errors more precisely (can be phase-later).

Feature flags:

* `default = ["wasi"]`
* `wasi`: enables `wasi::Errno` conversions (should be on in Wasmer builds)
* `host-errno` (optional): enables `cfg(unix)` host errno helpers (for `vfs/host`)

---

## Public API surface

### 1) Error mapping (`errno.rs`)

**Purpose:** unify all VFS→WASI error mapping.

#### Required types (from `vfs/core`)

This spec assumes `vfs/core` provides:

* `VfsError { kind: VfsErrorKind, source: Option<Box<dyn Error + Send + Sync>> , ... }`
* `VfsErrorKind` is a stable enum (the plan lists examples)

`vfs/unix` must *not* define these; it consumes them.

#### Functions

```rust
/// Convert a VFS error to a WASI errno (single source of truth).
pub fn vfs_error_to_wasi_errno(err: &vfs_core::VfsError) -> wasi::Errno;

/// Convert a VFS error kind to WASI errno (useful in tests and fast paths).
pub fn vfs_error_kind_to_wasi_errno(kind: vfs_core::VfsErrorKind) -> wasi::Errno;
```

#### Mapping table (must be explicit and documented)

A junior dev should implement this as a `match` with comments. Suggested baseline:

| `VfsErrorKind`    | WASI errno                                                              |
| ----------------- | ----------------------------------------------------------------------- |
| NotFound          | `NOENT`                                                                 |
| NotDir            | `NOTDIR`                                                                |
| IsDir             | `ISDIR`                                                                 |
| AlreadyExists     | `EXIST`                                                                 |
| PermissionDenied  | `ACCES`                                                                 |
| NotSupported      | `NOTSUP` *(or `NOSYS` if truly “unimplemented”; pick one and document)* |
| InvalidInput      | `INVAL`                                                                 |
| TooManySymlinks   | `LOOP`                                                                  |
| NameTooLong       | `NAMETOOLONG`                                                           |
| DirNotEmpty       | `NOTEMPTY`                                                              |
| ReadOnlyFs        | `ROFS`                                                                  |
| CrossDevice       | `XDEV`                                                                  |
| Busy              | `BUSY`                                                                  |
| WouldBlock        | `AGAIN`                                                                 |
| Interrupted       | `INTR`                                                                  |
| Overflow          | `OVERFLOW`                                                              |
| BadFileDescriptor | `BADF`                                                                  |
| SeekOnPipe        | `SPIPE`                                                                 |
| NoSpace           | `NOSPC`                                                                 |
| FileTooLarge      | `FBIG`                                                                  |
| TooManyOpenFiles  | `MFILE`                                                                 |
| TooManyLinks      | `MLINK`                                                                 |
| Stale             | `STALE` *(if you include it)*                                           |
| Io                | `IO`                                                                    |
| Unknown           | `IO` *(fallback)*                                                       |

**Important ties to other phases:**

* Phase 3 cross-mount rename must become `XDEV`. That should happen by `vfs/core` returning `VfsErrorKind::CrossDevice`, and then this mapping returns `wasi::Errno::XDEV`.
* Overlay “lower-only dir rename not supported” must become `NOTSUP` (or another explicitly-chosen errno). Choose once here and reuse everywhere.

#### Optional helpers (nice-to-have)

```rust
/// For logging/telemetry only: stable small integer or static str for error kind.
pub fn vfs_error_kind_str(kind: vfs_core::VfsErrorKind) -> &'static str;
```

---

### 2) Open flag translation (`open_flags.rs`)

**Purpose:** prevent `lib/wasix` and `vfs/host` from each implementing their own flag conversion.

#### Assumed internal VFS type

This spec assumes `vfs/core` defines something like:

```rust
pub struct OpenOptions {
  pub read: bool,
  pub write: bool,
  pub append: bool,
  pub truncate: bool,
  pub create: bool,
  pub create_new: bool,
  pub directory: bool,
  pub no_follow: bool,
  pub nonblock: bool,
  pub sync: bool,
  pub dsync: bool,
  // plus any “path resolution” hints that core needs
}
```

If `vfs/core` doesn’t yet have this, create it there (Step 1.2/2.x) and treat this file as the authoritative bridge from WASI → that struct.

#### Required functions

```rust
/// Convert WASI/WASIX open-like inputs into VFS OpenOptions.
///
/// Inputs should mirror Wasix syscall parameters:
/// - oflags: create/trunc/excl/directory flags
/// - fdflags: append/nonblock/sync
/// - lookupflags: symlink-following rules etc (if applicable)
pub fn wasi_open_to_vfs_options(
  oflags: wasi::Oflags,
  fdflags: wasi::Fdflags,
  lookupflags: Option<wasi::Lookupflags>,
) -> vfs_core::OpenOptions;
```

If Wasix uses different flag types (common in WASIX extensions), add overloads but keep logic in one place.

#### Rules (must be documented in code comments)

* `O_CREAT` → `create = true`
* `O_EXCL` with `O_CREAT` → `create_new = true` (else ignored or InvalidInput; pick a behavior and test it)
* `O_TRUNC` → `truncate = true` (but only meaningful with write access; core/backends decide final enforcement)
* `FD_APPEND` → `append = true`
* `FD_NONBLOCK` → `nonblock = true` (core returns `WouldBlock` where supported; otherwise may ignore per capabilities)
* `LOOKUP_SYMLINK_FOLLOW` (or inverse) decides `no_follow`
* `O_DIRECTORY` or WASI equivalent sets `directory = true` and should yield `NOTDIR` if target isn’t a directory (enforced in `vfs/core` open semantics)

**Relationship to other plan parts:**

* Phase 2.1 symlink traversal behavior depends on `no_follow`/`lookupflags`. Wasix must pass those flags through rather than implementing symlink policy itself.
* Phase 2.4 OFD semantics: `append` is OFD-level state (in VFS handle), while `CLOEXEC` is FD-level state (Wasix resource table). This file must **not** handle CLOEXEC.
* Phase 6 hostfs: if hostfs uses native `openat`, it will translate `vfs_core::OpenOptions` into OS flags in `vfs/host` (or via a small helper here behind `cfg(unix)` if you prefer). The key is: Wasix → VFS options happens here, not twice.

#### Optional reverse mapping

If Wasix needs to expose fdflags back out:

```rust
pub fn vfs_handle_state_to_wasi_fdflags(h: &vfs_core::VfsHandle) -> wasi::Fdflags;
```

(Do this only if a concrete syscall needs it, e.g. `fd_fdstat_get`.)

---

### 3) Directory entry encoding (`dirents.rs` + `filetype.rs`)

**Purpose:** avoid each backend and Wasix writing its own `getdents` packing logic.

#### Inputs

Use VFS-level types:

* `vfs_core::VfsDirEntry { inode: VfsInodeId or u64, name: String/OsString, file_type: VfsFileType, cookie: u64? }`

If `vfs/core` doesn’t define a cookie model yet, define one now:

* The encoder takes entries in a deterministic iteration order and emits a `next_cookie` to resume later.

#### Filetype mapping (`filetype.rs`)

```rust
pub fn vfs_filetype_to_wasi(ft: vfs_core::VfsFileType) -> wasi::Filetype;
```

#### Dirent ABI (pick and freeze one)

Wasix typically implements a WASI-flavored `fd_readdir` buffer format (not Linux `dirent64`). Whatever Wasix currently expects, codify it here.

If you need a concrete format now, use WASI `dirent` layout:

* fields: `d_next: u64`, `d_ino: u64`, `d_namlen: u32`, `d_type: u8`, then name bytes
* alignment: WASI commonly aligns to 8

Define constants in code:

```rust
pub const WASI_DIRENT_HEADER_SIZE: usize = 8 + 8 + 4 + 1 + 3; // if padding is applied
pub const WASI_DIRENT_ALIGN: usize = 8;
```

#### Encoder API

```rust
/// Encodes directory entries into `dst` in WASI dirent format.
///
/// Returns:
/// - bytes_written
/// - next_cookie (for the next call)
/// - number_of_entries_written
pub fn encode_wasi_dirents(
  entries: impl Iterator<Item = vfs_core::VfsDirEntry>,
  start_cookie: u64,
  dst: &mut [u8],
) -> EncodeDirentsResult;

pub struct EncodeDirentsResult {
  pub bytes_written: usize,
  pub next_cookie: u64,
  pub entries_written: u32,
}
```

#### Encoder rules (must be tested)

* Never write a partial dirent; if the next entry doesn’t fit, stop and return what you wrote.
* `start_cookie` means “skip entries until cookie >= start_cookie”. (Or “skip N entries”, depending on your cookie model—choose one and document.)
* Output must be deterministic for a fixed entry sequence.
* Names are bytes; if your VFS names are UTF-8 `String`, this is easy. If you use `OsString`, decide on an encoding strategy:

  * For Wasix, strongly prefer storing names as bytes/UTF-8 in VFS core for portability.
* Filetype conversion must be centralized (`filetype.rs`).

**Relationship to other plan parts:**

* Phase 2/3 path walking and mounts determine *which directory* you read; this module determines how the results are returned to Wasix.
* Overlay merged readdir order rules (upper first, then lower, no sorting required) must be preserved by whatever iterator you pass into this encoder; the encoder must not reorder entries.

---

### 4) Optional: IO error normalization (`io_error.rs`)

This is helpful for `vfs/host` so it can produce consistent `VfsErrorKind` from OS errors.

```rust
/// Best-effort conversion from std::io::Error to VfsErrorKind.
///
/// Used by vfs/host to normalize platform-specific errors.
pub fn io_error_to_vfs_error_kind(e: &std::io::Error) -> vfs_core::VfsErrorKind;
```

* On Unix, if you can extract raw errno, map common errnos (`ENOENT`, `ENOTDIR`, `EACCES`, `EEXIST`, `EXDEV`, …).
* On Windows, start with `ErrorKind` mapping; raw OS code mapping can be phase-later.

This keeps “host error normalization” out of `vfs/core` and avoids duplicating mapping logic across backends.

---

## Integration points (how other phases should use 1.4)

### Phase 2 (core semantics)

* `vfs/core` returns `VfsErrorKind` values that are **semantic**, not “host errno-shaped”.
* `vfs/unix::errno` maps those to WASI errno exactly once.
* Any syscall-level code in Wasix must call `vfs_error_to_wasi_errno()` and avoid custom mapping.

### Phase 3 (mounts/overlay)

* Overlay and mount code must choose `VfsErrorKind` consistently:

  * cross-mount rename → `CrossDevice`
  * unsupported overlay ops → `NotSupported`
* That guarantees stable Wasix-visible behavior via the mapping table here.

### Phase 5 (Wasix integration)

Wasix’s new FS shim (`lib/wasix/src/fs/vfs.rs`) should:

* Translate syscall flags with `wasi_open_to_vfs_options()`
* Convert returned errors with `vfs_error_to_wasi_errno()`
* Encode readdir buffers with `encode_wasi_dirents()`

…and **nothing else** in Wasix should implement these conversions.

### Phase 6 (hostfs)

Hostfs should:

* Normalize OS errors to `VfsErrorKind` with `io_error_to_vfs_error_kind()` (optional but recommended)
* Avoid defining its own errno translation tables.

---

## Testing requirements (acceptance criteria for Step 1.4)

### Build acceptance

* `cargo check -p vfs-unix` succeeds.
* No dependency cycle: `vfs-core` must not depend on `vfs-unix`.

### Unit tests (must exist)

1. **errno mapping coverage**

   * Table-driven test: each `VfsErrorKind` variant maps to the expected `wasi::Errno`.
   * Include the overlay/mount critical cases: `CrossDevice → XDEV`, `NotSupported → NOTSUP`, `TooManySymlinks → LOOP`.

2. **open flag translation**

   * Table-driven: WASI flags → `OpenOptions`.
   * Include combinations: `CREAT|EXCL`, `TRUNC`, `APPEND`, `NOFOLLOW`/lookupflags.
   * Add at least one “invalid combo” test if you decide to treat it as `InvalidInput`.

3. **dirent encoding**

   * Given known entries and a small buffer, ensure:

     * encoder stops before partial writes
     * `next_cookie` advances correctly
     * byte layout matches the expected header fields + name bytes
     * alignment/padding rules are correct

### Doc acceptance

* Module-level docs in `errno.rs`, `open_flags.rs`, and `dirents.rs` explaining:

  * “Do not duplicate this elsewhere”
  * what is guaranteed vs best-effort

---

## Implementation notes for a junior developer (practical guidance)

* Keep everything **pure** and easy to test: prefer `match` + small helper functions over clever abstractions.
* Put the mapping tables in **one place** (one `match` per mapping), and keep them exhaustive.
* Don’t add new behavior in Wasix or hostfs “just for now”—if it’s translation, it belongs here.
* If you discover Wasix uses slightly different flag types than expected, **adapt the function signatures**, but keep all conversion logic in `vfs/unix`.

---

## Deliverables checklist (Step 1.4)

* [ ] `vfs/unix/src/lib.rs` with clean re-exports
* [ ] `errno.rs` with `vfs_error_to_wasi_errno()` + comprehensive mapping
* [ ] `open_flags.rs` with `wasi_open_to_vfs_options()`
* [ ] `filetype.rs` mapping VFS file types to WASI file types
* [ ] `dirents.rs` with `encode_wasi_dirents()` and a frozen byte layout
* [ ] Unit tests for all three areas
* [ ] Short README or module docs stating: “This is the only place for these translations.”

If you want, I can also include a concrete `OpenOptions` and `VfsErrorKind` definition that fits the rest of the plan, but I didn’t assume new core types beyond what Step 1.4 needs to reference.
