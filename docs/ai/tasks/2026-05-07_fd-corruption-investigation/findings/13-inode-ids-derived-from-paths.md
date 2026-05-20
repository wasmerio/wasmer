# Finding 13: Inode IDs are derived from paths/names

## Verdict

INVALID for the fd-corruption investigation.

This code does derive `Inode` values from path/name strings, but in the current tree that hash is mostly metadata plus a write-only side table. The actual filesystem behavior that decides which object an fd points at is driven by `InodeGuard` objects stored directly in directory-entry maps and fd-table entries, not by looking up the hashed inode ID. I did not find a path where this suspect alone can make writes land on the wrong file descriptor or the wrong backing file.

## Relevant code

- `lib/wasix/src/fs/mod.rs:126-128`
  - `Inode::from_path()` is `xxh64(path_or_name, 0)`.
- `lib/wasix/src/fs/mod.rs:1747-1778`
  - `create_inode_with_stat()` chooses an `inode_key` from the file/dir path, symlink tuple, or fallback `name`, then sets `stat.st_ino` from `Inode::from_path(&inode_key)`.
- `lib/wasix/src/fs/mod.rs:296-318`
  - `WasiInodes::add_inode_val()` uses `stat.st_ino` as the `lookup` key and inserts `Weak<InodeVal>` into `WasiInodesProtected.lookup`.
- `lib/wasix/src/fs/mod.rs:1919-1922`
  - `remove_inode()` removes from that `lookup` map.
- `lib/wasix/src/fs/mod.rs:1091-1297`
  - Path traversal uses `Dir.entries: HashMap<String, InodeGuard>` and returns/clones those `InodeGuard`s directly.
- `lib/wasix/src/syscalls/wasix/path_open2.rs:268-399` and `:486-538`
  - Opening an existing path uses the resolved `InodeGuard`; creating a new file creates a new inode and stores that object in the parent directory's `entries`.
- `lib/wasix/src/syscalls/wasi/path_link.rs:95-131`
  - Hard links are represented by inserting `source_inode.clone()` into the target parent directory. They do not depend on `Inode::from_path()`.
- `lib/wasix/src/syscalls/wasi/path_rename.rs:157-308`
  - Rename moves the existing `InodeGuard` between parent `entries` maps and updates its stored `path`; it does not re-key the inode.

## What is actually true

### 1. `st_ino` is path/name-derived, not backend-inode-derived

That part is real.

- Real files/directories use the current host path string as the key.
- Symlinks use `"{base_po_dir}:{path_to_symlink}"`.
- Non-path synthetic objects fall back to the provided `name`.

So this is not a monotonic inode allocator, and it is not using host/backend inode identities.

### 2. The global inode `lookup` table can be overwritten by duplicate hashes

That part is also real.

`WasiInodes::add_inode_val()` does:

- read `stat.st_ino`
- build `Inode(st_ino)`
- `lookup.insert(ino, Arc::downgrade(&val))`

So if two inode objects share the same hashed ID, the later one overwrites the earlier weak entry.

However, I could not find a read-side that uses this table for path resolution, fd lookup, open, close, rename, unlink, or write routing. In this tree:

- `add_inode_val()` writes the table,
- `remove_inode()` removes from it,
- and `remove_inode()` itself has no call sites in `lib/wasix/src`.

That makes the overwrite largely inert today.

## Why this does not look like an fd-corruption primitive

### 1. FDs hold `InodeGuard`s directly

`Fd` stores `inode: InodeGuard` in `lib/wasix/src/fs/fd.rs`. Once an fd exists, operations go through that object, not through an inode-number lookup.

### 2. Directory traversal uses directory-entry pointers, not inode-number lookup

`get_inode_at_path_inner()` walks `Dir.entries` and returns the stored `InodeGuard`. Missing paths are materialized from the backing filesystem and then inserted into `entries`. The hashed inode ID is not consulted during lookup.

### 3. Hard links and renames keep sharing/moving the same inode object explicitly

- `path_link_internal()` inserts `source_inode.clone()` into the target directory.
- `path_rename_internal()` removes an entry from one parent directory and inserts that same `InodeGuard` into the target parent.

So aliasing between names is implemented by shared object identity, not by hash equality.

### 4. No write path I found resolves a file target by hashed inode ID

The suspicious corruption class here would be "two unrelated files end up sharing lookup identity, so an fd or write lands on the wrong target." I did not find such a path. The write/open/rename/link logic is all object-reference-based.

## Real issues that remain, but are not this bug

### 1. Metadata quality is weak

This implementation gives WASI-visible inode numbers that are:

- not backend inode numbers,
- potentially unstable across lifecycle changes,
- and potentially reused for unrelated synthetic objects with the same fallback name.

Examples of repeated fallback names exist, e.g. `"socket"` and `"event"` passed to `create_inode_with_default_stat()` in:

- `lib/wasix/src/syscalls/wasix/sock_open.rs:101`
- `lib/wasix/src/syscalls/wasix/sock_accept.rs:155`
- `lib/wasix/src/syscalls/wasi/fd_event.rs:50`

That means `st_ino` quality is not strong and may be non-unique for synthetic objects. This is a correctness/metadata concern, not an fd-routing concern.

### 2. Rename can leave inode numbers path-history-dependent

`path_rename_internal()` moves the existing `InodeGuard`, so the inode object keeps its old `ino()` after rename. Later stat paths often report the guard's stored inode number:

- `path_filestat_get_internal()` resolves the inode and then overwrites `stat.st_ino` with `file_inode.ino().as_u64()` in `lib/wasix/src/syscalls/wasi/path_filestat_get.rs:75-89`.
- `fd_filestat_get()` returns the stored inode stat in `lib/wasix/src/fs/mod.rs:1554-1558`.

So `st_ino` can be path-history-dependent rather than reflecting the current path or a backend inode. That is surprising metadata, but still not wrong-fd corruption.

### 3. There is one collision-sensitive correctness check in `path_rename`

`path_rename_internal()` short-circuits to success when:

- `source_parent_inode.ino() == target_parent_inode.ino()`
- and the entry names are equal

at `lib/wasix/src/syscalls/wasi/path_rename.rs:143-146`.

If two distinct parent directories ever produced the same `xxh64` value, a rename between them with the same basename could incorrectly no-op. That is a real theoretical bug, but:

- it depends on an actual 64-bit hash collision,
- it affects rename semantics, not fd/file-target corruption,
- and it is not enough by itself to explain the fd-corruption investigation.

## Bottom line

This suspect is real as a design smell for inode metadata, but I do not think it is a real fd-corruption cause in the current implementation.

The important distinction is:

- `Inode::from_path()` influences `st_ino` and a mostly-unused side map,
- while actual fd/file behavior is controlled by `InodeGuard` object references stored in fd entries and directory-entry maps.

Unless another bug starts using `WasiInodesProtected.lookup` for live resolution, or unless someone demonstrates a deliberate `xxh64` collision hitting one of the few equality checks, this suspect should stay marked INVALID for the corruption root-cause search.
