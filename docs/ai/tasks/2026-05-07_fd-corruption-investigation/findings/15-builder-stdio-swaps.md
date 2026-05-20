# Suspect 15: `Builder/setup stdio swaps can replace stdio handles after FS creation`

Verdict: INVALID

## Executive summary

`WasiEnvBuilder::build_init()` does create the default stdio descriptors first and then replaces their backing `VirtualFile`s with `swap_file()`. That part of the suspicion is real. However, in the code investigated here, this is an intentional stdio-redirection mechanism, not an fd-corruption bug by itself.

The key point is that `swap_file()` for fd `0`, `1`, and `2` does not replace the fd-table entry or redirect some hidden stale pointer. It swaps the boxed file handle inside the existing stdio inode, so later users of stdout/stderr/stdin consistently resolve the new handle through the normal stdio paths. I did not find a path in this suspect where writes meant for one descriptor are silently routed to a different descriptor.

There is one real semantic mismatch: after swapping stdio to a different backing object, `fd_fdstat_get(0/1/2)` and `fd_filestat_get(0/1/2)` still report the hard-coded stdio metadata (`CharacterDevice`, default rights/flags). That is surprising and may matter for metadata-sensitive code, but it is not enough to classify this suspect as fd corruption.

## Relevant code path

1. `WasiFs::new_with_preopen()` always creates default stdio descriptors first.
   - `lib/wasix/src/fs/mod.rs:797-800`
2. `WasiEnvBuilder::build_init()` then swaps in configured stdio handles.
   - stdin override/default: `lib/wasix/src/state/builder.rs:915-918`
   - post-FS swap calls: `lib/wasix/src/state/builder.rs:960-978`
3. `swap_file()` for stdio only swaps the `VirtualFile` stored in the existing stdio inode.
   - `lib/wasix/src/fs/mod.rs:966-1016`
   - stdio helper accessors used by `swap_file()`: `lib/wasix/src/fs/mod.rs:321-347`, `350-392`
4. The stdio inode/fd metadata comes from `create_std_dev_inner()`.
   - `lib/wasix/src/fs/mod.rs:2177-2221`

## What `swap_file()` actually changes

For stdio fds, `swap_file()` does this:

- fd `0`: `WasiInodes::stdin_mut(&self.fd_map)?.swap(file)`
- fd `1`: `WasiInodes::stdout_mut(&self.fd_map)?.swap(file)`
- fd `2`: `WasiInodes::stderr_mut(&self.fd_map)?.swap(file)`

That means:

- The fd number stays the same.
- The `Fd` entry in `fd_map` stays the same.
- `Fd.is_stdio` stays `true`.
- The inode identity stays the same (`FS_STDIN_INO`, `FS_STDOUT_INO`, `FS_STDERR_INO`).
- The inode `name` stays `"stdin"`, `"stdout"`, or `"stderr"`.
- The inode `Kind::File.fd` stays `Some(0|1|2)`.

Only the boxed `VirtualFile` inside the existing stdio inode is replaced.

This is important because it means the stdio descriptor is not being destroyed and recreated as some unrelated file descriptor. Consumers that resolve stdout/stderr later will still hit fd `1`/`2`; they will just see the new backing handle.

## Why this does not look like fd corruption

### 1. Builder-time setup is synchronous and finishes before execution starts

In `build_init()`, the stdio swaps happen before the method returns the initialized environment, and before any guest code runs:

- `WasiFs` is created.
- configured stdio handles are swapped in;
- optional `setup_fs_fn` runs;
- then `build_init()` continues constructing the environment.

There is no async boundary or guest execution window in this section that would let one thread use the old handle while another silently observes a different fd number.

Relevant code:

- `lib/wasix/src/state/builder.rs:952-980`

### 2. Normal stdio consumers resolve the current handle, not a stale old one

The main consumers examined all go back through the current stdio inode/handle:

- `fd_write` gets the current `Fd`, then the current inode handle, and writes through that handle.
  - `lib/wasix/src/syscalls/wasi/fd_write.rs:128-226`
- `fd_read` does the same for reads.
  - `lib/wasix/src/syscalls/wasi/fd_read.rs:135-229`
- runtime helper `stderr_write()` uses `WasiInodes::stderr_mut(&state.fs.fd_map)` directly, so it also follows the current fd 2 backing.
  - `lib/wasix/src/syscalls/mod.rs:246-255`
- `WasiState::stdout()/stderr()/stdin()` wrap the current fd entry and then re-lock the current inode handle on use.
  - `lib/wasix/src/state/mod.rs:225-252`
  - `lib/wasix/src/fs/inode_guard.rs:463-552`

So after a swap, later writers/readers consistently see the swapped handle. I did not find a hidden side channel that keeps writing to the pre-swap stdio backing after the swap completes.

### 3. `/dev/stdout`-style opens intentionally follow the current stdio descriptor

Stdio inodes are created with `Kind::File { fd: Some(raw_fd), ... }`.
`path_open2` treats such files as special: it short-circuits to the stored fd, and if a handle reports a special fd, it clones that fd.

Relevant code:

- stdio inode setup: `lib/wasix/src/fs/mod.rs:2187-2221`
- special-file short circuit and clone path: `lib/wasix/src/syscalls/wasix/path_open2.rs:279-350`

This means `/dev/stdout` and similar paths are deliberately tied to the current fd `1`/`2` semantics. After a stdio swap, those paths following the replacement is consistent with the rest of the system, not evidence of corruption.

### 4. Later `swap_file()` users are intentional redirection sites

Repo-wide `swap_file()` callers are limited and all look deliberate:

- builder initialization:
  - `lib/wasix/src/state/builder.rs:962-973`
- DCGI instance reuse reattaches request/response pipes:
  - `lib/wasix/src/runners/dcgi/factory.rs:78-93`
- DCGI recycle swaps in `NullFile` placeholders:
  - `lib/wasix/src/runners/dcgi/callbacks.rs:43-59`
- one fd-close test installs a blocking replacement file:
  - `lib/wasix/src/syscalls/wasi/fd_close.rs:417-425`

The DCGI caller comments explicitly say stdio is reattached on each call because it is consumed to EOF during nominal flows. That is a redirection/reuse design, not an accidental late corruption path.

## Evidence against the specific corruption theory

The suspicion said the late swap might happen "without enough metadata changes for other paths to notice."

That is partly true about metadata, but not about the actual data path:

- The metadata *is* mostly preserved on purpose.
- The backing handle *does* change.
- The write/read paths I traced use the backing handle, so they *do* notice.

In other words: there is metadata staleness, but not stale-target I/O in the paths inspected.

## Real issue found: stdio metadata becomes misleading after swap

Two functions continue to hard-code stdio metadata by fd number:

- `fdstat(0|1|2)` always returns `CharacterDevice` and default stdio rights/flags.
  - `lib/wasix/src/fs/mod.rs:1560-1585`
- `filestat_fd(fd)` returns the cached inode `stat`.
  - `lib/wasix/src/fs/mod.rs:1554-1558`
- stdio inodes are initialized with `Filestat { st_filetype: CharacterDevice, ... }`.
  - `lib/wasix/src/fs/mod.rs:2187-2203`

Since `swap_file()` only swaps the handle, not the inode `stat` or fd metadata, a swapped stdout/stderr can still present itself as the original stdio character device even if the actual handle is now a pipe or some other file-like object.

That is a semantic mismatch worth noting for future cleanup, but it does **not** by itself explain wrong-fd writes or descriptor corruption.

## Concurrency / race notes

I did not find a race in this suspect itself:

- builder-time swaps happen before the environment is returned;
- `swap_file()` mutates the existing stdio handle under the handle lock, rather than replacing the fd-map entry;
- the existing fd-close regression test shows explicit care around stdio replacement and delayed flushes.
  - `lib/wasix/src/syscalls/wasi/fd_close.rs:400-476`

If a future investigation finds corruption involving live, concurrent host-side `swap_file()` calls while guest code is running, that would be a separate suspect centered on synchronization/lifecycle guarantees for those callers. Nothing in this builder/setup path alone demonstrates such corruption.

## Bottom line

This suspect is **INVALID** as an fd-corruption root cause.

What is real:

- stdio is created first and then its backing handle is swapped;
- `swap_file()` can intentionally redirect stdio later too;
- stdio metadata is not updated to match the new backing object.

What I did **not** find:

- replacement of fd `1`/`2` with some unrelated fd-table entry;
- a stale writer path that keeps targeting the old handle after the swap;
- a builder/setup timing issue that alone can reroute writes to the wrong descriptor.
