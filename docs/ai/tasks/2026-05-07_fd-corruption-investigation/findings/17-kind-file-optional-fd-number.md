# Suspect 17: `Kind::File` stores an optional fd number

## Verdict

INVALID

The stored inode-level fd is genuinely stale-prone, and there is one real consumer of it in `path_open_internal()`. But in the current repository, that consumer is only reachable for `Kind::File` inodes whose `fd` field was set to `Some(...)`, and the only in-tree producers of such inodes are:

1. `create_std_dev_inner()`, which creates stdio inodes at fd `0/1/2` but does **not** insert them into any directory tree, so pathname lookup cannot reach them.
2. `open_file_at()`, which does insert a `Kind::File { fd: Some(real_fd) }` into directory entries, but a codebase search shows no in-tree callers of `open_file_at()`.

So this field is a latent API hazard, not an active in-repo fd-corruption source.

## Relevant code

### 1. The inode type really stores a raw fd number

`Kind::File` carries:

- `handle: Option<...>`
- `path: PathBuf`
- `fd: Option<u32>`

Reference:

- `lib/wasix/src/fs/fd.rs:89-100`

The field comment explicitly says it is for "special file[s]" that "should be looked up by path", which is exactly the kind of cross-layer numeric-fd state that can go stale.

### 2. `path_open_internal()` really consumes that stored fd

When `path_open_internal()` finds an existing inode and that inode is `Kind::File { fd: Some(special_fd), .. }`, it immediately returns that numeric fd instead of opening a new descriptor:

- `lib/wasix/src/syscalls/wasix/path_open2.rs:279-287`

That means the suspect is not purely dead data. There is a real fast path:

1. resolve path to an existing inode
2. inspect `Kind::File.fd`
3. if `Some(fd)`, return that fd directly

If such an inode were reachable by pathname after its stored number became stale, later opens could resolve to the wrong current descriptor.

### 3. The field becomes stale immediately under normal fd-table operations

The field is not maintained by normal fd lifecycle code:

- `create_fd_ext()` allocates/inserts fd-table entries but does not update `Kind::File.fd`:
  - `lib/wasix/src/fs/mod.rs:1828-1870`
- `clone_fd_ext()` duplicates an fd-table entry and shares the same inode, but does not update `Kind::File.fd`:
  - `lib/wasix/src/fs/mod.rs:1877-1908`
- `close_fd()` removes the fd-table entry and does not clear `Kind::File.fd`:
  - `lib/wasix/src/fs/mod.rs:2294-2312`

So the basic stale-fd concern is correct:

1. inode stores fd `N`
2. `N` is closed, duplicated, or replaced
3. inode still stores `N`
4. later code that trusts `Kind::File.fd` may resolve to the wrong current fd-table entry, or to `Badf`

## Where `fd: Some(...)` is produced

### 4. `open_file_at()` creates path-reachable inodes with `fd: Some(real_fd)`

`open_file_at()`:

1. creates `Kind::File { handle: Some(...), path: "", fd: None }`
2. inserts that inode into the parent directory entry map
3. allocates a descriptor with `create_fd(...)`
4. mutates the inode to `fd: Some(real_fd)`

References:

- creation with `fd: None`: `lib/wasix/src/fs/mod.rs:913-917`
- insertion into directory entries: `lib/wasix/src/fs/mod.rs:924-929`
- post-allocation mutation to `Some(real_fd)`: `lib/wasix/src/fs/mod.rs:934-953`

If this API were exercised, the bug shape would be real:

1. `open_file_at()` creates a file inode whose pathname resolves to a stored fd number
2. that fd number later becomes stale after close/reuse/dup/renumber
3. `path_open_internal()` reopens the pathname and returns the stale/reused number instead of creating a fresh descriptor

That is not just cosmetic metadata; it would be wrong behavior.

### 5. But `open_file_at()` appears unused in this repository

A repository-wide search for `open_file_at(` found only its definition in `lib/wasix/src/fs/mod.rs`.

I did not find any in-tree call path that creates these path-reachable `Kind::File { fd: Some(...) }` inodes.

That matters because without `open_file_at()`, the main stale-fd scenario above is not exercised by the current codebase.

### 6. The other producer is stdio setup, but those inodes are not path-reachable

`create_std_dev_inner()` creates stdio inodes as:

- `Kind::File { fd: Some(raw_fd), handle: Some(...), path: "" }`

and then only:

1. adds the inode to `WasiInodes`
2. inserts an `Fd` into `fd_map` at `raw_fd`

References:

- `lib/wasix/src/fs/mod.rs:2187-2221`

Crucially, this code does **not** insert the inode into any `Dir.entries` or `Root.entries` map. So pathname lookup cannot reach these `fd: Some(0|1|2)` inodes.

That removes the main concern that stale stdio numbers here might directly affect `/dev/stdout`-style opens.

## Why `/dev/stdout` and friends are not using this field

The active special-file path for `/dev/stdin`, `/dev/stdout`, and `/dev/stderr` is separate:

1. the virtual-fs special file reports a constant fd via `get_special_fd()`
   - `lib/virtual-fs/src/special_file.rs:13-27`
   - `lib/virtual-fs/src/special_file.rs:99-101`
2. `path_open_internal()` clones that current fd-table entry through `handle.get_special_fd()`
   - `lib/wasix/src/syscalls/wasix/path_open2.rs:337-349`

So the repo's real `/dev/stdout` behavior is driven by the file handle's `get_special_fd()`, not by `Kind::File.fd`.

That is important because it means the suspect is **not** the mechanism behind current special-file opens.

## Bottom line

The underlying intuition was good: storing a process-local fd number in inode state is unsafe because fd numbers are reusable and the field is not kept in sync with close/dup/renumber operations.

However, in the current repository this does **not** appear to be an active corruption source:

- the one path-reachable producer (`open_file_at()`) has no in-tree callers
- the in-tree stdio producer creates unreachable inodes from the pathname layer
- active `/dev/stdout`-style opens use `handle.get_special_fd()` instead

So this suspect should stay `INVALID` for the current investigation.

## Guidance for a later fix agent

This field is still worth cleaning up because it is a misleading latent hazard:

1. If `open_file_at()` is ever used, it should not encode reopen semantics by storing a numeric fd in the inode.
2. If inode-level special-file identity is needed, it should be modeled as a stable semantic marker (for example "current stdout") rather than a reusable fd number.
3. If the field is no longer needed, deleting it and the `path_open_internal()` fast path would remove a future footgun.
