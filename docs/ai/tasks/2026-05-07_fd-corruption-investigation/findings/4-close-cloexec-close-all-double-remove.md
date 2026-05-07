# Finding 4: `close_cloexec_fds` / `close_all` double-remove after `close_fd`

## Verdict

VALID

This suspect is a real fd-table corruption bug, not just benign cleanup redundancy.

`WasiFs::close_fd()` already removes the descriptor from `fd_map`. Both cleanup helpers then mutate the map again later using stale information:

- `close_cloexec_fds()` calls `map.remove(fd)` a second time for the originally collected numbers.
- `close_all()` calls `map.clear()` after already calling `close_fd(fd)` for every collected number.

Because the helpers are async and do not hold the `fd_map` lock across the whole cleanup, any concurrent fd allocation into the same `WasiFs` can be deleted by the second mutation.

## Relevant code

- `lib/wasix/src/fs/mod.rs:654-687` - `close_cloexec_fds()`
- `lib/wasix/src/fs/mod.rs:690-711` - `close_all()`
- `lib/wasix/src/fs/mod.rs:2304-2323` - `close_fd()`
- `lib/wasix/src/fs/fd_list.rs:176-191` - `FdList::remove()`
- `lib/wasix/src/fs/fd_list.rs:67-84` - `FdList::insert_first_free()`
- `lib/wasix/src/fs/mod.rs:1838-1880` - `create_fd_ext()` allocates via `insert_first_free()`
- `lib/wasix/src/state/env.rs:1277-1304` - exit cleanup awaits `close_all()` before signaling/terminating the process
- `lib/wasix/src/os/task/process.rs:834-842` - `terminate()` has `FIXME: this is wrong, threads might still be running!`
- `lib/wasix/src/bin_factory/exec.rs:143-149` - exec pre-run calls `close_cloexec_fds().await`
- `lib/wasix/src/state/env.rs:221-244` - `WasiEnv::clone()` shares `Arc<WasiState>`
- `lib/wasix/src/syscalls/wasix/proc_exec3.rs:277-302` - `proc_exec3()` clones the current env and spawns using it
- `lib/wasix/src/syscalls/wasix/thread_spawn.rs:137-181` - threads are spawned from cloned env/state, so multiple live threads can share one `WasiFs`

## What the code does

### `close_fd()` already removes the entry

`WasiFs::close_fd()` takes `fd_map.write()`, executes `fd_map.remove(fd)`, and returns. There is no deferred cleanup token or tombstone; the fd slot is immediately free again.

`FdList::remove()` updates `first_free`, so the slot becomes eligible for immediate reuse. `create_fd_ext()` with `idx = None` allocates with `insert_first_free()`, which prefers the lowest freed slot.

That means:

1. `close_fd(fd)` makes the numeric fd reusable immediately.
2. A later `map.remove(fd)` is operating on stale assumptions.
3. A later `map.clear()` removes every descriptor that happens to exist at that moment, including descriptors created after the cleanup loop started.

## Why `close_cloexec_fds()` is buggy

`close_cloexec_fds()`:

1. snapshots a `HashSet<WasiFd>` from the map,
2. for each fd: `flush(fd).await`, then `close_fd(fd)`,
3. reacquires `fd_map.write()` and does `map.remove(fd)` again for the same stale fd numbers.

The second `remove(fd)` is unsafe because `close_fd(fd)` already freed that number.

### Corruption interleaving

One concrete race:

1. Thread A enters `close_cloexec_fds()` during exec pre-run and snapshots fd `7` in `to_close`.
2. Thread A runs `self.close_fd(7)`. Slot `7` is now free.
3. Thread B, sharing the same `WasiFs`, performs any fd-creating operation (`path_open`, `fd_dup`, `sock_accept`, pipe creation, etc.). `create_fd_ext()` / `clone_fd_ext()` can reuse slot `7`.
4. Thread A finishes the loop, reacquires `fd_map.write()`, and executes `map.remove(7)`.
5. The new descriptor in slot `7` is silently deleted.

This is independent of any flush correctness issue. The stale second removal is enough by itself once another thread can allocate.

### Is that concurrency actually reachable?

Yes.

The important exec path is not isolated:

- `spawn_exec_module()` installs a pre-run hook that does `state.fs.close_cloexec_fds().await`.
- `proc_exec3()` builds that new execution from `ctx.data().clone()`.
- `WasiEnv::clone()` keeps the same `Arc<WasiState>` and same `WasiProcess`, so the new exec thread shares the same `WasiFs`.
- WASIX supports additional threads via `thread_spawn`, also based on cloned env/state.

I did not find any checkpoint/freeze/barrier around `close_cloexec_fds()` that would stop other threads from issuing syscalls against the shared `WasiFs` while the pre-run cleanup is awaiting.

So `close_cloexec_fds()` is a real stale-fd-number race in multithreaded/shared-state exec scenarios.

## Why `close_all()` is buggy

`close_all()` is even stronger:

1. snapshots all current fd numbers,
2. for each fd: `flush(fd).await`, then `close_fd(fd)`,
3. then unconditionally `map.clear()`.

Once `close_fd(fd)` has run, any later allocation into the map is outside the original cleanup set. The final `map.clear()` still destroys it.

### Corruption interleaving

1. Thread A starts `close_all()`.
2. Thread A closes fd `5`, freeing that slot.
3. Thread B opens a new descriptor while cleanup is still running.
4. Thread A reaches the final `map.clear()`.
5. Thread B's brand-new descriptor is removed, even if its fd number was not in the original `to_close` set.

Unlike `close_cloexec_fds()`, `close_all()` does not even require fd-number reuse for the final corruption. Any descriptor inserted before the final `clear()` is lost.

### Is that concurrency actually reachable?

Yes, and the caller makes it particularly clear.

`WasiEnv::blocking_on_exit()` does:

1. await `state.fs.close_all()`,
2. then `process.signal_process(Signal::Sigquit)`,
3. then `process.terminate(process_exit_code)`.

So fd cleanup runs **before** the process is even signaled for termination.

Also, `WasiProcess::terminate()` contains an explicit comment:

> `FIXME: this is wrong, threads might still be running!`

That is strong evidence the codebase does not guarantee process-wide quiescence before or during `close_all()`. If another thread is still live during exit cleanup, the final `map.clear()` can wipe out descriptors it just opened.

## Dependency on other suspects

This finding does **not** depend on a separate bug.

- Immediate fd-number reuse comes from the intended `FdList` behavior, not from a distinct defect.
- For `close_all()`, reuse is not even required; the final `clear()` can delete any newly inserted descriptor.

So this suspect is sufficient on its own to cause fd-table corruption when concurrent users of the same `WasiFs` exist.

## Scope / limitations

- In a single-threaded startup path with no concurrent fd allocation, the redundant second mutation may be harmless in practice.
- That does **not** make the code benign overall, because reachable shared-state paths (`proc_exec3`, process-exit cleanup) do allow concurrent access.

## Bottom line

The bug is real:

- `close_fd()` already removes the entry.
- `close_cloexec_fds()` later removes by stale fd number.
- `close_all()` later clears the whole table.
- Existing shared-state threading/exec/exit paths make concurrent allocation plausible and, for `close_all()`, directly supported by caller ordering.

This is a genuine candidate for "new descriptor disappears / wrong file ends up at reused fd number" corruption.

## Notes for a follow-up fix agent

Likely fixes to evaluate:

1. Remove the final `map.remove(fd)` in `close_cloexec_fds()` and the final `map.clear()` in `close_all()`.
2. If a second pass is needed for bookkeeping, track object identity or keep the map locked for the entire critical section instead of acting on stale fd numbers.
3. Re-check caller-side synchronization for exec/exit cleanup; if cleanup assumes process quiescence, that assumption is currently not enforced.
