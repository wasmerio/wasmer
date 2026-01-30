# Wasmer Virtual FS Report (virtual-fs + wasix fs)

## Scope
This report reviews the current virtual filesystem implementation in:
- `lib/virtual-fs`
- `lib/wasix/src/fs`

It documents flaws and Linux-compatibility gaps, then proposes a clean-slate design inspired by operating systems. It closes with a survey of third‑party crates that might be reused or mined for patterns.

## Executive Summary
The current stack is a mix of path-based virtual FS traits, multiple ad‑hoc filesystem implementations (mem, host, overlay, union), and a WASIX inode/FD layer that caches path-based entries. The system lacks critical Linux semantics: stable inode identity, real symlink and hardlink behavior, permissions/ownership, atomicity guarantees, and correct overlay/union semantics. Several parts are explicitly labeled as hacks or broken, and some components (overlay, union, host fs) diverge from Linux behavior in ways that will show up as user-visible incompatibilities.

A Linux-compatible design should be inode-centered, include a VFS layer with mount tree and dentry cache, model open file descriptions separately from file descriptors, and implement overlay/union semantics per Linux overlayfs rules (whiteouts, opaque directories, copy-up, metadata copy-up). It should also enforce rights/permissions consistently and provide a stable interface for symlinks, hardlinks, device nodes, xattrs, and file locks.

## Current Architecture (High-Level)
- **`virtual-fs`**: Provides `FileSystem` and `VirtualFile` traits, plus implementations (mem, host, overlay, union, tmp, etc.). Most operations are path-based, not inode-based, and metadata is minimal.
- **`wasix` FS layer**: Maintains an inode table and FD table for WASI/WASIX syscalls. It lazily populates inodes by resolving paths against a `FileSystem` and caches entries in memory.
- **Overlay/Union**: `OverlayFileSystem` is a simple primary+secondary chain with `.wh.` whiteouts; `UnionFileSystem` mounts sub‑filesystems by the first path component.

## Analysis of Current Flaws and Support Gaps

### 1) Core Abstractions Are Path-Based, Not Inode-Based
Linux semantics are inode-centric: hard links, rename, and open file descriptors all rely on stable inode identity. The current `FileSystem` interface only supports path-based operations and has no concept of inode handles or link counts. This leaks into WASIX, which derives inode IDs from path hashes and stores paths in `Kind::File/Dir`.

Consequences:
- Hardlinks cannot be correct (confirmed by TODO: hard links broken with rename).
- Rename invalidates inode identity and breaks any path‑based references.
- Multiple paths to the same inode cannot be represented accurately.

### 2) Symlink Support Is Partial and Semantically Incomplete
- `virtual-fs` explicitly states that `symlink_metadata` is identical to `metadata` because symlinks aren't implemented. This removes key Linux behavior (`lstat`, symlink traversal semantics, symlink permissions/ownership).
- WASIX path resolution partially models symlinks but: absolute symlinks are explicitly unsupported, and there is a note that symlink following is “super weird” (even when `follow_symlinks` is false). This produces non‑Linux behavior.

### 3) Permissions, Ownership, and Xattrs Are Missing
`Metadata` contains only file type and timestamps; no UID/GID, mode bits, link count, device numbers, or xattrs. There is no chmod/chown, no umask, no ACLs. WASIX rights checks are mentioned as TODOs and are inconsistent.

### 4) Overlay/Union Semantics Diverge from Linux
`OverlayFileSystem` uses `.wh.` file name prefixes to hide lower entries. This is not Linux overlayfs semantics, which use special whiteouts and “opaque” directory markers. The implementation:
- Lacks opaque directories, so lower entries can “leak” into an upper directory that should be opaque.
- Uses regular files for whiteouts, not character device whiteouts.
- Has no copy-up of metadata without data (e.g., chmod/chown or timestamps), and copy-on-write logic only copies data.
- Does not preserve metadata (mode, uid/gid, xattrs, times) on copy-up.
- `OverlayFileSystem::mount` is unsupported, so nested mount behavior is incomplete.

`UnionFileSystem` mounts only by first path component and does not implement standard Linux mount table semantics. It cannot properly model overlapping or shadowed mounts.

### 5) Host FS Semantics Differ from Linux
- `host_fs::rename` uses copy+delete across directories when moving between parents; this is not atomic and does not match Linux `rename(2)` semantics.
- `read_dir` uses `entry.metadata()` rather than `symlink_metadata()`, so readdir’s file type for symlinks is inconsistent with Linux.
- Sandbox enforcement is path-based; symlinks inside a sandboxed root can escape to host paths in the absence of capability checks.

### 6) MemFS Has Known Broken Behavior
`CustomFileNode` is explicitly marked broken: `VirtualFile` stores its own offset, so a file stored this way can only be read once. This is a correctness issue, not just a limitation.

Additionally, MemFS doesn’t implement symlinks, hardlinks, or link counts, and uses a simplistic inode allocation (slab index) without robust link semantics.

### 7) Inode Cache Coherency and Staleness
The WASIX inode tree caches `Dir` entries and file paths. If the backing filesystem changes underneath, the cache can be stale, leading to incorrect metadata, missing entries, or incorrect file type in subsequent accesses.

### 8) Path Handling Is Inconsistent
The existence of `RelativeOrAbsolutePathHack` indicates inconsistent path expectations between different filesystem backends (some require absolute paths, some allow relative). This is not Linux-like and makes it hard to reason about sandboxing and mount boundaries.

### 9) Error Semantics and Errno Coverage
`FsError` is a reduced mapping of `io::ErrorKind` and does not capture a full set of Linux errnos. Key Linux behaviors (EXDEV for cross-device rename, ELOOP for symlink loops, EISDIR, ENOTDIR, ENOTEMPTY) are not consistently enforced.

### 10) Missing Linux File System Features
Major Linux features absent or incomplete:
- Hard links, proper symlink creation/handling, and `lstat`.
- POSIX permissions/ownership, sticky bit, setuid/setgid.
- Extended attributes and ACLs.
- Device nodes and special files (creation, permission enforcement).
- File locks and `fcntl`/`flock` semantics.
- `statfs`/`statvfs` and mount flags.
- Proper inode numbers, link counts, and stable file IDs.
- Copy-on-write correctness (sparse files, reflinks, metadata-only copy-up).
- Correct semantics for `rename`, `unlink` on open files, and directory removal rules.

### 11) Heterogeneous Backends (Host/Network/Object Store) Not First-Class
The current design does not explicitly model the fact that Wasmer must wrap multiple backend filesystems (host FS on Linux/Windows/macOS, networked FS, and object stores like S3). These backends have **different semantic guarantees** and **capability gaps** (e.g., object stores typically lack POSIX rename, hardlinks, or symlinks). The current interface does not expose a capability matrix or a consistency model, so the upper layers cannot make correct or predictable choices based on backend capabilities.

This results in two practical problems:
- **Semantic ambiguity**: APIs imply Linux semantics even when the backend cannot provide them.
- **Inconsistent behavior**: The same operation can behave differently depending on backend FS, without being visible to callers.

## Clean‑Slate Design Proposal (Inspired by Operating Systems)

### 1) Adopt a VFS Layer with Explicit Inodes
Model the filesystem after Linux/BSD VFS:
- **Superblock / filesystem instance** per mounted filesystem.
- **Inode** as the canonical identity of a file (stable across rename).
- **Dentry cache** for path resolution with negative dentries.
- **Mount tree** with per‑mount options and namespaces.

### 2) Separate “Open File Description” from FD
Introduce a `FileDescription` object (like Linux `struct file`) that:
- Holds file position, open flags, and references to inode and mount.
- Supports shared offsets across duplicated FDs.
- Allows per-FD flags separate from open flags.

### 3) Full Linux Path Resolution Semantics
Implement path resolution with:
- `.` / `..` traversal consistent with mount boundaries.
- Absolute vs relative paths based on a per‑process current working directory.
- `openat`‑style operations as the canonical interface.
- Symlink resolution limits with correct `ELOOP` behavior.

### 4) Proper Overlay/Union Semantics
Implement overlayfs semantics close to Linux:
- Upper + lower layers, with xattrs for whiteouts and opaque directories.
- Copy‑up on first write or metadata change.
- Preserve ownership, mode, timestamps, and xattrs on copy-up.
- Respect mount points and hidden directories.

### 5) First‑Class Backend Integration (Host/Network/Object Store)
Wrapping multiple filesystems is the primary use‑case; the architecture should make it explicit:
- **Backend adapter trait**: a lower layer for “filesystem providers” with explicit capability flags (atomic rename, hardlinks, symlinks, unix perms, xattrs, locks, case sensitivity, etc.).
- **Consistency model**: explicit contracts (strong, close‑to‑open, eventual, read‑after‑write) so cache invalidation and coherence are predictable.
- **Semantic tiers**: POSIX‑grade tier (local host FS), limited‑POSIX tier (network FS), object‑store tier (S3/GCS/Azure). The VFS should either (a) **emulate** missing features with caveats or (b) **surface errors** reliably and early.

This does **not** make inode‑based semantics untenable. Instead, the VFS can maintain **virtual inode identities** that map to backend objects and survive renames and path changes where possible. For backends without stable inode IDs (e.g., object stores), inode IDs can be synthetic and tied to object keys plus versioning/ETags, but the VFS must document that some Linux guarantees (hardlinks, atomic rename, link counts, directory semantics) cannot be fully preserved. Object-store‑backed filesystems are explicitly non‑POSIX in practice (e.g., Mountpoint for S3 does not support symlinks, file locking, or modifying existing files).

### 5) Permissions and Identity
Implement POSIX permission checks and metadata:
- UID/GID, mode bits, link counts, device IDs.
- Umask and per‑process credentials.
- Optional support for ACLs and xattrs.

### 6) Coherency and Consistency
- Use inode versioning to validate cached dentries.
- Provide cache invalidation or watch hooks for host‑backed FS.
- For host passthrough, use capabilities or `openat`-style relative to a dir fd to prevent escape.

### 7) Compatibility Test Suite
Establish a Linux compatibility suite:
- Reuse POSIX test suites where possible.
- Add focused tests for overlay and mount behavior.
- Include WASI-specific behavioral tests.

## Are Inode‑Based Semantics Compatible with Heterogeneous Backends?
Yes, but only with explicit layering and capability disclosure.

Key points:
- **Virtual inode IDs**: VFS can assign stable internal inode IDs even when backends don’t. For host filesystems, you can use native file IDs. For object stores, use `(bucket, key, version/etag)` to build stable-ish identities and invalidate on change. This is common in virtualization stacks.
- **Feature gating**: VFS should expose “this mount supports hardlinks/rename‑atomic/symlinks/xattrs” so callers can fail early or apply emulation.
- **Emulation vs. correctness**: Emulation (e.g., hardlinks via reference counting + copy-on-write) can be offered for limited cases, but Linux‑exact semantics are not always possible on object stores or certain networked backends.

Therefore inode‑centric design is still the correct foundation; it just needs a **capability‑aware** mapping layer for non‑POSIX backends, rather than pretending all backends are POSIX filesystems.

## Analysis of Third‑Party Crates (Potential Reuse)

### FUSE / Virtio‑FS Stacks
- **`fuse-backend-rs`**: Virtualization‑oriented FUSE backend with internal VFS/pseudo‑fs layer and virtio‑fs transport support. Useful as a reference for VFS structure and virtio‑fs integration.
- **`fuser`**: Mature Rust FUSE implementation; good for exposing Wasmer FS to the host or building adapters. Does not provide Linux‑complete semantics by itself.

### VFS/Overlay Abstractions
- **`vfs`** crate: Provides a simple VFS abstraction with overlay support. Useful as a reference for API ergonomics, but too high‑level and not Linux‑compatible.
- **`axfs_vfs`** (ArceOS): Small VFS trait set (VfsOps/VfsNodeOps), useful to study OS‑style layering and device/ramfs integration, but lacks symlink support and Linux‑grade semantics.

### Capability‑Based Sandboxing
- **`cap-std`**: Capability‑based filesystem API with `Dir` handles and OS support across Linux/macOS/Windows. Strong candidate for safe host passthrough and path confinement.

### OS/Filesystem Implementations in Rust
- **RedoxFS**: Full filesystem implementation (COW, Unix attributes) with FUSE compatibility; good reference for inode semantics and metadata.

### Filesystem Format Libraries (Read‑Only / Image‑Based)
- **`squashfs_reader` / `squinter`**: Read‑only SquashFS access; useful for image-based rootfs or package layers.
- **`fatfs`**: FAT filesystem implementation in Rust. Potentially useful for disk images or compatibility layers.
- **`littlefs2`**: Embedded filesystem; less relevant for Linux compatibility but good for resilient storage in constrained environments.

### Object Store / Cloud Backends
- **`object_store` (Apache Arrow)**: A high‑performance async object store API (S3, GCS, Azure, local, HTTP/WebDAV). It explicitly **does not** try to be a filesystem interface, which is important for correctness.
- **`OpenDAL`**: Data access layer for many storage services. Can be paired with `object_store` via `object_store_opendal` to reach more backends. Useful for unified backend access, but still object‑store semantics.
- **Mountpoint for S3**: Shows real‑world behavior of an object‑store filesystem (limited POSIX support; no symlinks, no locks, limited write semantics). This is a good reference for what can and cannot be emulated.

### Takeaways on Reuse
- There is **no drop‑in Rust library** that already provides Linux‑grade VFS semantics with overlays plus heterogeneous backends (host + network + object store). The ecosystem provides **building blocks**, not a full solution.
- The most reusable **conceptual** assets are in `fuse-backend-rs` (virtio‑fs/VFS scaffolding), `cap-std` (capability‑based host access), and OS‑level crates like RedoxFS/axfs_vfs (inode semantics and VFS layering).
- For cloud/object storage, use `object_store` and/or OpenDAL for access, but treat them as **non‑POSIX backends** and model their limitations explicitly.

## Closing Notes
Achieving “100% Linux compatibility” requires a fundamental shift to inode‑centric design, full metadata modeling, correct path resolution, and correct overlay semantics. The current architecture is serviceable for limited WASI/virtual use cases, but it structurally prevents correctness for hardlinks, symlinks, permissions, and overlay semantics. A clean‑slate VFS foundation aligned with OS models is the most reliable path to Linux‑grade behavior.
