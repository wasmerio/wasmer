# FS Refactor Requirements (Task 0.1)

## Objectives
- Deliver a Linux‑compatible, POSIX‑semantics virtual filesystem with correct path resolution, mount traversal, link behavior, permissions, file descriptor semantics, and metadata behavior.
- Support nested mounts, bind mounts, overlay/union mounts, and runtime mount/unmount with Linux‑like semantics.
- Integrate cleanly with Wasix resource handling while keeping filesystem logic in dedicated crates.
- Support heterogeneous FsProviders (host OS, in‑memory, network, object store) without kernel integration.
- Provide sync and async APIs for all filesystem operations, with efficient adapters between modes.
- Provide filesystem watching with provider‑native integration or VFS‑level emulation when unsupported.
- Enforce usage limits (bytes, inodes, file size, entries) and rate limits (IOPS/throughput) with reusable limiter abstractions.
- Prioritize performance: low‑allocation hot paths, efficient maps, and minimal contention.

## Non‑Negotiable Semantics
- **Path resolution**: `.`/`..` handling, symlink resolution with loop limits, trailing slash behavior, `openat`/`AT_FDCWD` semantics, and mount boundary traversal must match Linux.
- **Mounts**: Nested and overlay mounts supported; `..` across mounts behaves like Linux; mountpoints are per‑namespace with clear root behavior.
- **Open file descriptions**: Shared offsets and flags across duplicated FDs (`dup`/`dup2`) and per‑FD flags (`O_CLOEXEC`) must match POSIX.
- **Metadata**: `stat` fields and mode bits accurate; `chmod`, `chown`, `utimensat` semantics respected.
- **Rename/link**: Atomic rename semantics where possible; correct cross‑mount behavior; hardlinks and symlinks follow POSIX.
- **Permissions**: Enforce uid/gid/mode checks with mount‑level policy hooks.
- **Async + sync**: Both are first‑class, with explicit trait variants; adapters cannot change semantics.
- **Watching**: Must expose watcher APIs in Fs/FsProvider; VFS bridges events; polling fallback if provider lacks watch capability.
- **Limits**: Per‑Fs and per‑mount quotas; enforced in overlay upper layer to bound in‑memory growth.
- **Rate limiting**: Per‑mount, per‑Fs, and global throttling with wait‑until‑capacity semantics (async await + sync blocking) and optional non‑blocking errors.

## Required Capabilities
- FsProvider capability declaration (WATCH, HARDLINK, SYMLINK, RENAME_ATOMIC, SPARSE, XATTR, FILE_LOCKS, ATOMIC_O_TMPFILE, CASE_SENSITIVE, CASE_PRESERVING).
- VFS must gate or emulate behavior based on capabilities, including watcher emulation when WATCH is absent.
- Runtime provider registry with `register_provider` and `mount_with_provider`.

## Scope Boundaries
- **In scope**: VFS core semantics; provider abstractions; host/in‑memory/object store/network providers; overlay FS; limits and rate limiting; watcher API; Wasix resource integration.
- **Out of scope** (for initial cut): Full `ioctl` parity; complete device node emulation beyond minimal `dev` FS; platform‑specific extended attributes beyond a stable subset.

## Build Discipline
- After each step, crates must compile.
- Default: `cargo check --quiet --message-format=short`.
- If unclear: rerun with `cargo check --quiet` for full diagnostics.

