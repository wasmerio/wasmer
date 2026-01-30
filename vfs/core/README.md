# `vfs-core`

Semantic core types and contracts for the new Wasmer/Wasix VFS.

This crate intentionally contains:
- Core shared types (`VfsPath`, ids, metadata, errors, policy hooks).
- Provider-facing contracts used to create filesystem instances (`FsProvider` / `Fs`).

This crate intentionally does **not** contain (yet):
- Mount tables, overlays, or mount namespaces.
- Path walking / normalization / symlink traversal logic.
- OS-/WASI-specific flag translation or errno mapping (see `vfs-unix`).
- Async runtime coupling (see `vfs-rt`).

## Terminology

- `FsProvider`: filesystem type/driver (creates filesystem instances).
- `Fs`: mounted filesystem instance (superblock-like).
- `Mount`: binding of an `Fs` into the global VFS namespace (implemented later).

## Layering contract (early)

- `vfs-core` is the single place where semantic rules will live (path traversal, mount crossing,
  `..` behavior, symlink rules, OFD semantics, etc.).
- Backend crates (`vfs-host`, `vfs-mem`, `vfs-overlay`, …) should implement primitive operations
  on nodes/handles without relying on global absolute paths for correctness.
- Wasix and host/WASI interop map from `VfsError` to `wasi::Errno` in one place (`vfs-unix`).

