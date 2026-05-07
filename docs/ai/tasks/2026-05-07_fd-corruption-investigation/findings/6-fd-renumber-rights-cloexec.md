# Finding 6: `fd_renumber` rewrites rights and clears `CLOEXEC`

## Verdict

INVALID

This suspect is **not** a standalone fd-corruption root cause.

I found a real semantic inconsistency: `fd_renumber()` and the `proc_spawn2` `Dup2` path rebuild the destination `Fd` with `rights = source.rights_inheriting` instead of preserving `source.rights`. That can silently reduce the duplicated descriptor's capabilities and make later syscalls fail with `Errno::Access`.

However, the code still duplicates the same underlying inode/handle and shared offset. By itself, this does **not** create wrong-target writes, descriptor-table corruption, or fd-number confusion. The `CLOEXEC` clearing also appears intentional/expected for a dup2-like operation.

## Relevant code

- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:64-127` - `fd_renumber_internal()`
- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:206-247` - child `Dup2` operation
- `lib/wasix/src/fs/mod.rs:1877-1908` - `clone_fd_ext()` preserves `rights`
- `lib/wasix/src/fs/fd.rs:20-42` - `Fd` / `FdInner` layout
- `lib/wasix/src/syscalls/wasi/fd_fdstat_set_rights.rs:53-61` - rights can be reduced independently
- `lib/wasix/src/syscalls/wasi/fd_write.rs:142-145` - `FD_WRITE` enforced from `fd.inner.rights`
- `lib/wasix/src/syscalls/wasi/fd_read.rs:150-153` - `FD_READ` enforced from `fd.inner.rights`
- `lib/wasix/src/syscalls/wasi/fd_fdstat_set_flags.rs:44-49` - `FD_FDSTAT_SET_FLAGS` enforced from `fd.inner.rights`
- `lib/wasix/src/fs/mod.rs:654-680` - `CLOEXEC` is only consulted by `close_cloexec_fds()`
- `lib/wasix/src/bin_factory/exec.rs:144-149` - `close_cloexec_fds()` runs before exec
- `lib/wasix/src/fs/mod.rs:2187-2220` - stdio entries are created with `rights_inheriting = Rights::empty()`

## What the code does

### 1. `fd_renumber()` does not preserve the source descriptor's base rights

`fd_renumber_internal()` builds the replacement entry like this:

- shares `offset` with the source,
- clones the same `inode`,
- copies the rest of the `Fd`,
- but explicitly sets:
  - `rights = fd_entry.inner.rights_inheriting`
  - `fd_flags.CLOEXEC = false`

The relevant lines are:

- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:86-99`

So if the source has:

- base rights: `A`
- inheriting rights: `B`

then the destination gets:

- base rights: `B`
- inheriting rights: still `B`

That is different from ordinary fd duplication in this same codebase.

### 2. Ordinary `fd_dup` / `fd_dup2` preserve base rights

`WasiFs::clone_fd_ext()` is the normal duplication helper used by `fd_dup2`. It preserves both:

- `rights: fd.inner.rights`
- `rights_inheriting: fd.inner.rights_inheriting`

See:

- `lib/wasix/src/fs/mod.rs:1887-1906`
- `lib/wasix/src/syscalls/wasix/fd_dup2.rs:47-56`

That makes `fd_renumber()` and `proc_spawn2`'s `Dup2` path inconsistent with the repository's own main duplication helper.

### 3. `proc_spawn2` repeats the same rights rewrite

The child-fd operation path in `proc_spawn2.rs` builds the duplicated entry the same way:

- `rights = fd_entry.inner.rights_inheriting`
- clears `CLOEXEC`
- copies `inode`, `offset`, and the rest of the `Fd`

It even contains:

- `// TODO: verify this is correct`

See:

- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:229-243`

So the repo itself already signals uncertainty about this behavior.

## Why this is not fd corruption

### 4. The duplicated entry still targets the same underlying object

The duplicated `Fd` keeps:

- `inode: fd_entry.inode.clone()`
- `offset: fd_entry.inner.offset.clone()`

That means the destination descriptor still refers to the same underlying `Kind::File` / socket / pipe object and shares the same file-description-style offset state.

I did **not** find any renumber-time mutation that:

- points the destination at a different inode,
- changes a stored raw host fd number,
- corrupts the fd map entry itself,
- or makes later writes land on a different file than the source already referenced.

So this suspect does not explain "stdout/stderr data went to the wrong descriptor" by itself. It explains "the duplicated descriptor unexpectedly lost capabilities".

### 5. Observable consequence is capability loss, not descriptor confusion

Many syscalls enforce permissions from `fd.inner.rights`, for example:

- `fd_write()` requires `Rights::FD_WRITE`
- `fd_read()` requires `Rights::FD_READ`
- `fd_fdstat_set_flags()` requires `Rights::FD_FDSTAT_SET_FLAGS`

See:

- `lib/wasix/src/syscalls/wasi/fd_write.rs:142-145`
- `lib/wasix/src/syscalls/wasi/fd_read.rs:150-153`
- `lib/wasix/src/syscalls/wasi/fd_fdstat_set_flags.rs:44-49`

Therefore the direct effect of this bug is:

1. source fd has `rights != rights_inheriting`
2. `fd_renumber(from, to)` runs
3. `to` now has weaker base rights than `from`
4. later syscalls on `to` can fail with `Errno::Access`

That is a real semantic bug, but it is not "fd corruption".

## `CLOEXEC` assessment

### 6. Clearing `CLOEXEC` on the duplicated descriptor appears intentional

`Fdflagsext::CLOEXEC` is only used to decide what `close_cloexec_fds()` closes during exec:

- `lib/wasix/src/fs/mod.rs:654-680`
- `lib/wasix/src/bin_factory/exec.rs:144-149`

`clone_fd_ext()` already supports explicitly setting the duplicated descriptor's `CLOEXEC` bit through the `cloexec: Option<bool>` parameter:

- `lib/wasix/src/fs/mod.rs:1877-1908`

That matches ordinary dup-family behavior where a newly duplicated descriptor does not automatically inherit close-on-exec unless a specific CLOEXEC variant is requested.

So the `CLOEXEC` clearing in:

- `lib/wasix/src/syscalls/wasi/fd_renumber.rs:90-94`
- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:234-238`

looks expected, not suspicious.

## Important edge case: stdio

### 7. Stdio makes the rights bug easier to trigger, but still not corrupting

Stdio entries are created with:

- `rights_inheriting: Rights::empty()`

See:

- `lib/wasix/src/fs/mod.rs:2208-2219`

So duplicating/renumbering from a stdio source through this path can produce a destination whose base rights become empty.

But `fd_read()` / `fd_write()` explicitly bypass the `rights` check for `is_stdio` entries:

- `lib/wasix/src/syscalls/wasi/fd_read.rs:148-153`
- `lib/wasix/src/syscalls/wasi/fd_write.rs:140-145`

Since `fd_renumber()` copies the source `Fd` wholesale after overriding only a few fields, the destination also preserves `is_stdio` when the source was stdio.

Result:

- the metadata becomes odd,
- some rights-gated operations may behave unexpectedly,
- but this still does not redirect the descriptor to the wrong file/object.

## Minimal reproduction for the real bug

This is the smallest useful reproduction sketch for a later fix agent:

1. Create or obtain a descriptor `from`.
2. Use `fd_fdstat_set_rights(from, base, inheriting)` to make:
   - `base` include an operation such as `FD_WRITE`
   - `inheriting` omit that operation
3. Call `fd_renumber(from, to)` or trigger the `proc_spawn2` `Dup2` path.
4. Attempt the operation through `to`.

Expected if duplication preserved descriptor rights:

- `to` should behave like `from`

Actual with current code:

- `to` behaves as if its base rights were reduced to `from.rights_inheriting`
- writes/reads/flag changes may fail with `Errno::Access`

This reproduces a capability regression, not fd corruption.

## Dependency on other suspects

This finding does **not** depend on another bug to exist.

But to become "fd corruption", it would need a separate mechanism, such as:

- stale fd-number reuse,
- double-removal from the fd map,
- async close/flush races,
- or inode/handle aliasing bugs.

Without one of those separate defects, this suspect alone only changes descriptor metadata/capabilities.

## Bottom line

The precise verdict for the suspect as written is:

- `rights` rewrite: probably a real bug
- `CLOEXEC` clearing: expected/benign
- fd corruption / descriptor confusion: **not demonstrated by this code path alone**

So this suspect should be treated as **INVALID for the fd-corruption investigation**, while preserving a note that `fd_renumber()` and `proc_spawn2` likely have a separate rights-preservation bug worth fixing later.
