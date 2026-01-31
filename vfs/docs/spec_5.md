# Spec 5 — Wasix integration (syscalls + resource table)
  
## Status
  
- **Target plan section**: `fs-refactor.md` → **Phase 5 – Wasix Integration (syscalls + resource table)**
- **Important note from USER**: **Phase 6 is already completed**, so `vfs-*` providers exist and Phase 5 should integrate with them cleanly.
- **Primary integration target**: `lib/wasix` (crate `wasmer-wasix`).
  
**Hard requirement (updated)**: Phase 5 is a **hard cutover**. We must:
  
- **Remove** the legacy filesystem implementation under `lib/wasix/src/fs/*` that depends on `virtual_fs`
- **Remove all `virtual_fs` usage from the `wasmer-wasix` crate** (no `pub use virtual_fs`, no `VirtualFile`, no `virtual_fs::FileSystem`, no `UnionFileSystem`, etc.)
  
We **do** preserve the existing Wasix logic for non-filesystem resources (sockets, pipes, tty, epoll, notifications), but those parts must be refactored to no longer depend on `virtual_fs` types.
  
This spec is written to be executable by a junior engineer and to minimize churn in `lib/wasix` by **building around existing patterns**:
  
- `WasiEnvBuilder` builds `WasiEnvInit` (`lib/wasix/src/state/builder.rs`)
- runners (`lib/wasix/src/runners/*`) configure the builder and filesystem layout
- syscalls (`lib/wasix/src/syscalls/*`) access filesystem state through `WasiEnv.state.fs` and `fd_map`
  
---
  
## Goal (Phase 5 in one sentence)
  
Replace Wasix’s current path-resolution + filesystem plumbing (based on `virtual_fs` and custom inode trees) with the new `vfs-core` mount/path/handle semantics **and delete all `virtual_fs` usage from `wasmer-wasix`**, while preserving Wasix’s existing non-filesystem resources (sockets/tty/epoll/journal/etc.).
  
---
  
## Non-goals (explicitly out of scope for Phase 5)
  
- **No dual-stack**: we are not keeping `virtual_fs` around “temporarily”. Removing it is part of Phase 5.
- **No full Wasix syscall rewrite**. Only filesystem syscalls and the minimal support code that currently depends on `virtual_fs` must be rewritten.
- **No watch API integration** (VFS watch is optional and not required here).
- **No global architecture refactors** of unrelated subsystems (networking, journaling, asyncify, tasks, instance/linker).
  
---
  
## Glossary (keep these concepts straight)
  
- **Legacy FS** (to be deleted in Phase 5): `lib/wasix/src/fs/mod.rs` today, built around:
  - `WasiFsRoot` wrapping `virtual_fs::{TmpFileSystem, OverlayFileSystem, UnionFileSystem, FileSystem}`
  - `WasiInodes` + `InodeVal` + `Kind::*` with path-string traversal and cached entries
- **New VFS**: the clean-slate implementation under `vfs/`, primarily:
  - `vfs-core` mount table (`MountTable`) + path walking (`PathWalker`/`PathWalkerAsync`)
  - providers via registry (`FsProviderRegistry`)
  - correct OFD semantics via `VfsHandle` / `VfsHandleAsync`
  - errno mapping + open flags translation in `vfs-unix`
- **Wasix IO layer** (new in Phase 5): small internal types to replace `virtual_fs::VirtualFile` and `virtual_fs::pipe::*` for:
  - stdio (FD 0/1/2)
  - pipe endpoints and duplex pipes
  - any remaining places that used `virtual_fs::VirtualFile` purely as an async byte stream
  
- **`Kind` (Wasix FD resource type)**: the `enum Kind` in `lib/wasix/src/fs/fd.rs` is the Wasix equivalent of “what does this fd refer to?”.
  - **Phase 5 preserves this concept** (and most variants), but removes any `virtual_fs` backing types inside variants.
- **FD vs OFD**:
  - **FD**: per-descriptor flags like `CLOEXEC` (and in Wasix, rights bookkeeping)
  - **OFD**: shared offset + shared status flags across `dup()` (this is what `vfs-core::VfsHandle{,Async}` implements)
  
---
  
## What exists today in Wasix (important baseline)
  
### 1) Environment builder sets up legacy FS
  
`WasiEnvBuilder::build_init()` currently:
  
- chooses a `WasiFsRoot` (`Sandbox(TmpFileSystem)` by default, or `Overlay` when a container FS is present)
- constructs `WasiFs::new_with_preopen(...)`
- wires stdio as special file descriptors by calling `wasi_fs.swap_file(__WASI_STDIN_FILENO, ...)`, etc.
- stores `WasiFs` into `WasiState`
  
**Phase 5 change**: this entire construction path is removed. `WasiEnvBuilder` will instead build a `vfs-core` mount table and a Wasix FD table whose filesystem-backed entries reference `vfs-core` handles.
  
### 2) Runner assembles filesystem layout using `virtual_fs`
  
`CommonWasiOptions::prepare_webc_env()` currently:
  
- builds a `TmpFileSystem` root (via `RootFileSystemBuilder`)
- mounts user volumes (`MountedDirectory`) into it
- optionally overlays a `UnionFileSystem` from the package container (`BinaryPackage.webc_fs`)
- includes hacks:
  - **relative mount destination hack** (mount relative guest paths as `"/$path"`)
  - **RelativeOrAbsolutePathHack** to retry relative paths against `/` for webc volumes
  
**Phase 5 change**: all of this `virtual_fs`-based layout code is replaced by mounting providers into `vfs-core::MountTable`.
  
### 3) Packages are injected by “unioning” + dropping command stubs into `/bin`
  
`WasiEnv::use_package_async()` currently:
  
- merges (`conditional_union`) the package’s `webc_fs` into the root filesystem
- creates `/bin` and `/usr/bin` (best-effort)
- writes command atoms into `/bin/<cmd>` and `/usr/bin/<cmd>` (often via `insert_ro_file`)
- registers the binary mapping in `BinFactory` for process spawning
  
**Phase 5 change**: `BinaryPackage.webc_fs: Option<Arc<virtual_fs::UnionFileSystem>>` is removed from `wasmer-wasix`. Package filesystem visibility is achieved via a VFS provider (see “Package FS provider”).
  
### 4) Syscalls depend on the legacy `WasiFs` + `Kind::*` model
  
Example: `path_open_internal()` resolves paths by calling:
  
- `state.fs.get_inode_at_path(...)`
  
And `fd_read` / `fd_write` branch heavily on `Kind::File|Socket|Pipe|...` to decide behavior.
  
This means **Phase 5 must be careful**: we can’t rip out the entire `Kind` enum in one go without a large refactor.
  
---
  
## Phase 5 integration strategy (minimize refactors)
  
### High-level approach
  
1. **Delete the legacy filesystem stack** (`lib/wasix/src/fs/mod.rs` and related `virtual_fs`-based helpers).
2. Replace `virtual_fs`-dependent IO primitives (stdio, pipes) with a small internal IO layer.
3. Replace all filesystem syscalls to call into `vfs-core` path walking + node/handle APIs (async-first).
4. Update runners/builders to build the VFS mount layout (mount table) directly.
5. Update package loading/injection to use a VFS provider instead of `virtual_fs::UnionFileSystem`.
  
### Why this is the least invasive option
  
- We avoid rewriting the big Wasix subsystems:
  - networking, pipes, sockets, epoll, tty, journal replay logic
  - task manager integration and asyncify machinery
  
But note the constraint: removing `virtual_fs` means we *do* need to do targeted refactors of the IO types that previously came from `virtual_fs` (stdio + pipes). The resource logic remains; only the backing types change.
  
---
  
## How Linux models “VFS vs other FDs” (and how Wasix should mirror it)
  
This section is important because it explains why we preserve `Kind` instead of trying to force “everything is a file in the VFS”.
  
### Linux mental model
  
In Linux, a process has:
  
- a **single FD table**: integers \(0,1,2,...\)
- each FD points to a **struct file** (“open file description” in POSIX terms)
- the `struct file` contains:
  - a pointer to some underlying object (inode-backed file, socket, pipe, eventfd, epoll, etc.)
  - a pointer to that object’s **operations** (e.g. `file_operations` for inode-backed files, socket ops for sockets, etc.)
  
Crucially:
  
- **The VFS is only one producer of file objects**, specifically those backed by inodes in mounted filesystems.
- Many FDs are **not** VFS/inode-backed at all:
  - sockets (AF_INET, AF_UNIX, etc.)
  - pipes (`pipe()`), eventfd, signalfd, timerfd
  - epoll fds
  - `/proc` and `/dev` entries can be VFS-backed, but often backed by “special” filesystem drivers
  
Syscalls dispatch based on the FD’s underlying object:
  
- `read(fd)` works for regular files, pipes, sockets, etc., but with different semantics
- `readdir(fd)` only works when `fd` refers to a directory
- `path_openat(dirfd, path, ...)` walks the VFS mount tree; it doesn’t apply to sockets/pipes directly
  
### Wasix model (Phase 5 target)
  
Wasix should mirror Linux:
  
- Keep a **single FD table** (existing `fd_map` / `FdList` concept)
- Keep a **resource-type discriminator** (the `Kind` enum), but:
  - `Kind::File`/`Kind::Dir` become VFS-backed variants (`Kind::VfsFile`/`Kind::VfsDir`)
  - non-filesystem variants remain (Socket, Pipe*, Epoll, EventNotifications, Buffer, etc.)
  
Then implement syscall dispatch the same way Linux does:
  
- **Path syscalls** (`path_open`, `path_create_directory`, `path_unlink_file`, `path_readlink`, `path_rename`, etc.) route to `vfs-core` (mounts + path walker).
- **FD syscalls** (`fd_read`, `fd_write`, `fd_seek`, `fd_close`, `fd_fdstat_get`, etc.) do a `match kind` and route to:
  - VFS handles (for `Kind::VfsFile`/`Kind::VfsDir`)
  - socket implementation (for `Kind::Socket`)
  - pipe implementation (for `Kind::PipeRx`/`Kind::PipeTx`/`Kind::DuplexPipe`)
  - epoll implementation (for `Kind::Epoll`)
  - etc.
  
This is the core “hierarchy”: **VFS is a subsystem used by some FDs, not the owner of all FDs**.
  
---
  
## Deliverables (what to implement in Phase 5)
  
### 5.1 Define the new Wasix ↔ VFS interface surface
  
#### Deliverable: replace `lib/wasix/src/fs/` with a VFS-backed implementation
  
The old `lib/wasix/src/fs/mod.rs` is deleted. Replace it with a new filesystem module tree:
  
- `lib/wasix/src/fs/mod.rs` (new; thin re-exports + types)
- `lib/wasix/src/fs/vfs.rs` (new; VFS state + helpers)
- `lib/wasix/src/fs/fd_table.rs` (new; FD table, rights, CLOEXEC, etc.)
- `lib/wasix/src/fs/wasi_bridge.rs` (new; helper functions used by syscalls)
- `lib/wasix/src/fs/packages.rs` (new; package mounting + command injection)
- `lib/wasix/src/fs/stdio.rs` (new; stdio integration without `virtual_fs`)
- `lib/wasix/src/fs/pipes.rs` (new; pipe primitives without `virtual_fs`)
  
Rule: keep files small and focused, mirroring the style constraints from `fs-refactor.md` (“no huge files”).
  
This module tree must provide:
  
- **Path-based operations**:
  - `openat` / `statat` / `mkdirat` / `unlinkat` / `rmdir` / `renameat` / `readlinkat` / `symlinkat`
- **FD/OFD operations**:
  - `dup` (shares OFD), `close`, `seek`, `fdstat_get/set`, `filestat_get`, `read`, `write`
- **Conversions (single source of truth)**:
  - WASI flags → VFS flags using `vfs_unix::open_flags::wasi_open_to_vfs_options`
  - VFS errors → WASI errno using `vfs_unix::errno::vfs_error_to_wasi_errno`
  
#### Hard rule: do not re-implement path resolution in Wasix
  
All traversal (symlinks, `..`, mount transitions) must go through:
  
- `vfs_core::path_walker::PathWalker` (sync) or `PathWalkerAsync` (async)
  
Wasix is only allowed to:
  
- choose base dir (cwd vs a dir fd)
- choose resolve flags (nofollow, beneath/in_root policy, etc.)
- apply rights checks (WASI/WASIX rights gating)
  
### 5.2 Attach VFS handles to Wasix resources
  
#### Deliverable: redesign `Kind` to remove `virtual_fs` types
  
Replace `Kind` (currently in `lib/wasix/src/fs/fd.rs`) so it no longer references `virtual_fs::{VirtualFile, Pipe, PipeRx, PipeTx}`.
  
New minimum set of variants (illustrative; adjust names to match Wasix conventions):
  
- **Filesystem-backed**:
  - `Kind::VfsFile { handle: std::sync::Arc<vfs_core::VfsHandleAsync> }`
  - `Kind::VfsDir { handle: vfs_core::VfsDirHandleAsync }`
- **stdio** (replace `VirtualFile`-based stdio):
  - `Kind::Stdin { ... }`
  - `Kind::Stdout { ... }`
  - `Kind::Stderr { ... }`
- **pipes** (replace `virtual_fs` pipe types):
  - `Kind::PipeRx { ... }`
  - `Kind::PipeTx { ... }`
  - `Kind::DuplexPipe { ... }`
- **unchanged conceptual resources** (but refactor away any `virtual_fs` usage):
  - `Socket { socket: InodeSocket }`
  - `Epoll { ... }`
  - `EventNotifications { inner: Arc<NotificationInner> }`
  - `Buffer { buffer: Vec<u8> }` (if still used by syscalls)
  
Notes:
  
- Store the file handle as an `Arc<VfsHandleAsync>` so `dup` is just `Arc::clone()` and shares OFD state by construction.
- Store the directory handle as `VfsDirHandleAsync` (it is already cheaply cloneable and carries parent refs needed for mount-root `..`).
  
#### Deliverable: replace the stdio + pipe implementations
  
Because `virtual_fs` is removed, Phase 5 must introduce equivalents:
  
- `lib/wasix/src/fs/stdio.rs`:
  - Implement stdio as resources that can be read/written by `fd_read`/`fd_write`.
  - Recommended: use the existing runtime/console abstractions (`Runtime::tty()` and/or console types) rather than `VirtualFile`.
  - Keep journaling behavior (e.g. `skip_stdio_during_bootstrap`) by gating at syscall boundaries, not in the IO layer.
  
- `lib/wasix/src/fs/pipes.rs`:
  - Implement pipes using tokio primitives (e.g. `tokio::io::duplex()` plus split read/write halves) or a custom async pipe implementation.
  - Must support:
    - nonblocking reads/writes (return `Errno::Again` behavior)
    - readiness integration for epoll/polling via existing virtual-io interest handlers (keep the current *logic*, change the underlying types).
  
#### Deliverable: update syscall read/write/seek branches
  
Update `fd_read`, `fd_pread`, `fd_write`, `fd_pwrite`, `fd_seek`, `fd_close` to add a new match arm:
  
- When the inode kind is `Kind::VfsFile`, call the corresponding `VfsHandleAsync` method:
  - `read()` / `write()` for cursor-following operations
  - `pread()` / `pwrite()` for offset-based operations
  - `seek()` for `fd_seek`
- Keep existing behavior for:
  - `Kind::Socket`, `Kind::Pipe*`, `Kind::DuplexPipe`, `Kind::EventNotifications`, `Kind::Epoll`, tty, etc.
  
This keeps the “special resource” implementation stable and avoids risky refactors.
  
### 5.3 Ensure FD semantics match POSIX where applicable
  
Phase 5 must fix or avoid legacy correctness pitfalls:
  
- **Do not store open file state on the inode** for VFS-backed files.
  - Legacy `Kind::File { handle: Option<...> }` has known issues (commented in `fs/mod.rs` and `path_open2.rs`) and is not POSIX-correct for multiple opens.
  - The open handle must live on the FD resource (`Kind::VfsFile`) not “on the file”.
  
- **Dup shares offset**:
  - `Arc<VfsHandleAsync>` sharing guarantees OFD semantics.
  - Ensure `fd_dup` and `fd_renumber` (and journal replay equivalents) preserve the underlying `Arc<VfsHandleAsync>`.
  
- **CLOEXEC remains per-FD**:
  - Keep `Fdflagsext::CLOEXEC` in `FdInner`.
  - Do not put CLOEXEC into VFS handle status flags (it’s not an OFD property).
  
### 5.4 Wasix API bridging: builder, runner, package injection, volumes, stdio
  
This is where Phase 5 must integrate without breaking existing UX.
  
#### Deliverable: new filesystem layout builder used by runners
  
Add a helper (new file):
  
- `lib/wasix/src/fs/layout.rs`
  
It should build:
  
- a `vfs_core::provider_registry::FsProviderRegistry`
- a `vfs_core::mount::MountTable`
- a `vfs_core::context::VfsContext` (cred root by default)
  
And produce a `WasiFs` object stored in `WasiState` (see next section).
  
#### Deliverable: store VFS state in `WasiState`
  
Replace the `WasiState.fs` type so it is *the* VFS-backed filesystem state (no legacy field).
  
At the end of Phase 5:
  
- `WasiState.fs` must no longer contain:
  - `WasiFsRoot`, `TmpFileSystem`, `OverlayFileSystem`, `UnionFileSystem`
  - inode trees that store host `PathBuf` strings and cached entries
  
Instead, `WasiState.fs` should contain (suggested shape):
  
- `registry: Arc<vfs_core::FsProviderRegistry>`
- `mounts: vfs_core::MountTable`
- `ctx: vfs_core::VfsContext`
- `fd_table: ...` (Wasix FD table with rights + CLOEXEC + Kind variants)
- `preopens: Vec<...>` (as directory handles, not string aliases)
  
This is intentionally “single source of truth” so syscalls do not have to decide between legacy and new implementations.
  
#### Deliverable: adapt `WasiEnvBuilder` to optionally build the new VFS
  
In `lib/wasix/src/state/builder.rs`, remove `virtual_fs` construction entirely:
  
- `WasiEnvBuilder::build_init()` builds the VFS mount table and FD table directly.
- stdio is created as explicit `Kind::Stdin/Stdout/Stderr` resources (not `VirtualFile`).
- preopens are created as directory FDs that reference `vfs-core::VfsDirHandleAsync`.
  
#### Deliverable: adapt runners to mount volumes into the new VFS (no hacks)
  
In `lib/wasix/src/runners/wasi_common.rs`, stop building a `virtual_fs` root. Instead:
  
- Build mounts in the VFS mount table.
  
Mount rules:
  
- Guest mount destinations must be absolute. If user provides a relative destination, interpret it relative to `/` (to preserve existing CLI behavior) and log a warning.
- Use `vfs-host` to mount host directories and `vfs-mem` for the default writable upper layer.
  
- Register providers:
  - `mem` (`vfs-mem`)
  - `host` (`vfs-host`)
  - `overlay` (`vfs-overlay`)
  - (optional, depending on package FS approach) `webc` provider (see below)
  
- Replace the legacy “relative mount destination hack” with a deterministic rule:
  - **Rule**: guest mount destinations must be absolute. If user provides relative, interpret it relative to `/` (to preserve existing CLI behavior) and log a warning.
  
- For each volume mount:
  - Ensure the mountpoint directory exists in the upper filesystem (create parent dirs in `mem` upper).
  - Mount a `host` filesystem at that mountpoint using `MountTable::mount`.
  
#### Deliverable: packages injection in the new VFS (avoid recursive copies)
  
Phase 5 must handle two distinct “package injection” needs:
  
1. **Make package files visible in the FS namespace** (e.g. Python stdlib under `/lib/...`).
2. **Make package commands visible and spawnable** via:
   - filesystem stubs under `/bin/<cmd>` and `/usr/bin/<cmd>`
   - `BinFactory::set_binary("/bin/<cmd>", pkg)`
  
Recommended root layout in VFS:
  
- **`/` is an overlay mount**:
  - upper: `mem` (writable scratch)
  - lowers: one-or-more package layers (read-only)
  
This mirrors the semantics already implemented in `vfs-overlay` and avoids repeatedly copying entire package filesystems.
  
##### Package filesystem layer (required; no `virtual_fs` bridge allowed)
  
Because `wasmer-wasix` must not depend on `virtual_fs`, the `BinaryPackage.webc_fs` field is removed. Package filesystem content must be exposed to the VFS via a provider.
  
**Required approach**: implement a read-only `vfs-webc` provider and use it for package layers.
  
- Add a new crate `vfs/webc` (package name `vfs-webc`) implementing `FsProviderSync` named `"webc"`.
- It mounts a read-only view of a package filesystem layer without copying file data.
- The mount config should hold an immutable reference to the package’s volume(s) (e.g. `Arc<webc::Container>` and a resolved list of `(mount_path, volume)` mappings).
- It must implement:
  - `lookup`, `read_dir`, `metadata`, `open(read-only)` for regular files
  - `symlink` can be `NotSupported` initially unless webc supports it explicitly
- It must enforce `MountFlags::READ_ONLY` unconditionally (webc layers are immutable).
  
##### Command injection into `/bin` with new VFS
  
Implement in `lib/wasix/src/fs/vfs2/packages.rs`:
  
- Ensure directories exist:
  - `/bin`
  - `/usr/bin`
- For each `BinaryPackageCommand`:
  - Create `/bin/<name>` and `/usr/bin/<name>` as regular files in the **upper** (mem) layer
  - Write the command atom bytes
  - Set metadata mode to executable (e.g. `0o755`) when supported
  - Register in `BinFactory`:
    - `set_binary("/bin/<name>", pkg_arc)`
    - `set_binary("/usr/bin/<name>", pkg_arc)`
  
Important: this should not require the file to be read-only; correctness is “exists + contains atom bytes”. Read-only can be added later via mount policy or overlay rules.
  
#### Deliverable: stdio integration (FDs + optional `/dev` view)
  
Wasix already has robust stdio handling via `VirtualFile` and FD 0/1/2. Phase 5 must not break that.
  
- **Must keep**:
  - `stdin`, `stdout`, `stderr` as FDs 0/1/2
  - journaling special behavior (`skip_stdio_during_bootstrap`)
  
Additionally, many POSIX programs expect `/dev/stdin`, `/dev/stdout`, `/dev/stderr`.
  
Recommended minimal approach:
  
- Keep stdio as special FDs (not VFS-backed).
- Add an optional small VFS provider mounted at `/dev` that exposes:
  - `stdin`, `stdout`, `stderr` as special files whose `open()` returns handles wired to the existing stdio resources.
  
Implementation location:
  
- `lib/wasix/src/fs/vfs2/stdio.rs` can define a `DevFsProvider` (in-crate provider is fine; it does not need to live under `vfs/`).
  
If this is too much for the initial cut, document that `/dev/std*` is not supported yet and track it as a Phase 5 follow-up, but ensure FD 0/1/2 remain correct.
  
---
  
## Syscalls to migrate in Phase 5 (minimum viable set)
  
Phase 5 acceptance requires a minimal set of Wasix filesystem syscalls to use the new VFS. Because there is no legacy fallback, **all filesystem syscalls must be migrated** (or explicitly removed from exports).
  
- **Open / Read / Write / Close**
  - `path_open` (`lib/wasix/src/syscalls/wasi/path_open.rs`)
  - `path_open2` (`lib/wasix/src/syscalls/wasix/path_open2.rs`)
  - `fd_read`, `fd_pread` (`lib/wasix/src/syscalls/wasi/fd_read.rs`)
  - `fd_write`, `fd_pwrite` (`lib/wasix/src/syscalls/wasi/fd_write.rs`)
  - `fd_close` (file branch)
  
- **Stat**
  - `fd_filestat_get`, `path_filestat_get` / `path_filestat_get2` (where present)
  
- **Directory listing**
  - `fd_readdir` / `fd_fdstat_get` (depending on existing implementation)
  - or implement getdents path via `vfs-unix` helpers if used
  
- **Mutations**
  - `path_create_directory` (mkdir)
  - `path_remove_directory` (rmdir)
  - `path_unlink_file` (unlink)
  
All migrated syscalls must:
  
- perform Wasix rights gating before calling into VFS
- convert `VfsError` using `vfs_unix::errno::vfs_error_to_wasi_errno`
  
---
  
## Step-by-step implementation checklist (junior-friendly)
  
### Step 0: Make `wasmer-wasix` compile without `virtual_fs`
  
- Remove `pub use virtual_fs` and all `virtual_fs::*` imports from `lib/wasix`.
- Replace `VirtualFile`-based stdio and pipe types (see `fs/stdio.rs` and `fs/pipes.rs`).
- Replace package filesystem building that uses `virtual_fs::UnionFileSystem` (see `vfs-webc` provider).
  
Acceptance:
  
- `wasmer-wasix` has **no dependency on** `virtual-fs`/`virtual_fs` (direct or via re-export).
  
### Step 1: Introduce the new VFS-backed `WasiFs` state
  
- Implement the new `WasiFs` (VFS-backed) and store it in `WasiState`.
- Implement `layout::build_default_fs(...)` that:
  - registers providers
  - constructs a root overlay mount (upper=mem, lowers=package layers if any)
  - mounts host volumes at requested guest paths
  - sets initial cwd (default `/` or `/home` per runner logic)
  
Acceptance:
  
- a small unit test can create a `WasiEnv` with `vfs2` and list `/` using VFS.
  
### Step 2: Replace `Kind` + FD table for filesystem-backed resources
  
- Add `Kind` variants and plumb them through `InodeVal`/FD creation.
- Add helper functions in `fs/vfs2/wasi_bridge.rs`:
  - `insert_vfs_file_fd(...)`
  - `insert_vfs_dir_fd(...)`
  
Acceptance:
  
- `fd_close` can close a `Kind::VfsFile` without panicking.
  
### Step 3: Migrate `path_open` and `path_open2` (no legacy fallback)
  
- Implement a new open path:
  - determine base dir from `dirfd`
  - translate flags via `vfs_unix::open_flags::wasi_open_to_vfs_options`
  - apply read/write based on WASI rights
  - resolve + open using `PathWalkerAsync` and `FsNodeAsync::open`
  - wrap into `VfsHandleAsync`
  - install into fd table as `Kind::VfsFile`
  
Acceptance:
  
- A WASI module can `open()` and receive a valid fd.
  
### Step 4: Migrate `fd_read`/`fd_write` for VFS-backed files and stdio
  
- In `fd_read_internal` and `fd_write_internal`, add match arms:
  - `Kind::VfsFile` delegates to the `VfsHandleAsync`
  
Acceptance:
  
- A WASI module can write bytes to a file and read them back.
  
### Step 5: Migrate mkdir/rmdir/unlink + stat + readdir + rename + link + symlink
  
Because the legacy implementation is removed, cover the full WASI path+fd syscall surface:
  
- `path_filestat_get`, `path_filestat_set_times`, `fd_filestat_get/set_*`
- `path_link`, `path_symlink`, `path_readlink`
- `path_rename`
  
- Use `PathWalkerAsync` + node methods for:
  - `mkdir`
  - `unlink`/`rmdir`
  - `metadata` (stat)
  - `read_dir` (for directory fds)
  
Acceptance:
  
- A WASI module can create a directory, list it, create and delete a file inside it.
  
### Step 6: Integrate package injection on the new VFS path (via `vfs-webc`)
  
- When `WasiEnv::use_package_async()` runs under `vfs2`:
  - mount or attach the package layer(s) to the overlay lowers (or import once)
  - create `/bin/<cmd>` and `/usr/bin/<cmd>` in the overlay upper
  - call `bin_factory.set_binary()` as today
  
Acceptance:
  
- After injecting a package, `/bin/<cmd>` exists and `BinFactory` can spawn it.
  
### Step 7: Integrate volumes on the new VFS path (via `vfs-host`)
  
- Adapt `CommonWasiOptions::prepare_webc_env()` to additionally configure the new VFS mount table (under `vfs2`), using `vfs-host`.
  
Acceptance:
  
- The existing runner tests that mount a host volume and then `read_dir("/host")` continue to work (ported to VFS-backed code or dual-run).
  
---
  
## Acceptance criteria (Phase 5 “done”)
  
Phase 5 is considered complete when all are true:
  
- **`wasmer-wasix` contains no `virtual_fs` usage**:
  - no `virtual_fs` imports
  - no `pub use virtual_fs`
  - no `virtual_fs`-backed types in public APIs
- **Wasix filesystem syscalls are backed by `vfs-core`**:
  - open/read/write/close
  - stat
  - readdir
  - mkdir/rmdir/unlink
- **Packages integration works**:
  - injected package files are visible
  - injected commands appear under `/bin` and spawn works
- **Volumes work**:
  - host-mapped directories are mounted into the guest view without the legacy relative-path hack
- **FD/OFD semantics are correct for dup**:
  - duplicated fds share offsets for VFS-backed files (covered by a test)
- **No legacy stack remains**: the old `lib/wasix/src/fs/*` implementation and its helpers are deleted.
  
---
  
## Test plan (minimum required tests)
  
Add tests under `lib/wasix/tests/` (or `lib/wasix/src/.../tests`):
  
- **`vfs2_basic_file_io`**:
  - create file, write, read, close
- **`vfs2_dup_shares_offset`**:
  - open file, write, dup fd, read from dup and verify offset behavior matches POSIX/OFD expectations
- **`vfs2_mkdir_readdir_unlink`**:
  - mkdir, create file inside, readdir sees it, unlink, readdir updates
- **`vfs2_volume_mount_smoke`**:
  - mount host dir at `/host`, `readdir("/host")` works
- **`vfs2_package_injection_smoke`**:
  - load a small webc package, inject, assert `/bin/<cmd>` exists
  
Recommended command:
  
- `cargo test -p wasmer-wasix`
  
---
  
## Notes on known tricky areas (read before implementing)
  
- **BinaryPackage filesystem representation**:
  - Today it is `Arc<virtual_fs::UnionFileSystem>` in `wasmer-wasix`. Phase 5 removes this field and replaces it with a `vfs-webc` provider configuration.
  
- **Async vs sync**:
  - Wasix syscalls already use asyncify-like helpers; prefer using `FsNodeAsync` + `VfsHandleAsync` to avoid blocking mutexes in async contexts.
  
- **Don’t regress journaling**:
  - Keep journaling event recording at syscall boundaries (it records the syscall intent, not internal FS operations).
  
- **Avoid resurrecting “relative path hacks”**:
  - The new VFS should be the single path semantics implementation. If you find yourself prepending `/` to fix behavior, stop and fix the caller’s base-dir choice or the path walker flags.
  
