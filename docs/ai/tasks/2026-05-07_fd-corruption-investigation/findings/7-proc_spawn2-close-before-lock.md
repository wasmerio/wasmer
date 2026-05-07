# Finding 7: `proc_spawn2` dup2 closes target before taking the fd-map lock

## Verdict

INVALID for the suspected fd-corruption mechanism.

`proc_spawn2` does close the destination fd before reacquiring the `fd_map` write lock, and it does ignore the boolean result of `fd_map.insert(true, ...)`. But this happens while mutating a freshly forked child `WasiEnv`/`WasiFs` that has its own cloned `fd_map`, before the child process is launched. I did not find a path where another task can allocate into that child-local fd slot between `close_fd(op.fd)` and the later `insert(true, op.fd, ...)`.

The code is still worth cleaning up because:

- the ignored `insert()` result hides logic mistakes,
- source validation happens after closing the target, which is not ideal `dup2` behavior if `src_fd` is invalid,
- the implementation relies on isolation rather than lock-atomicity.

Those are correctness/maintainability concerns, but not enough by themselves to make this suspect a real fd-corruption source.

## Key evidence

### 1. `proc_spawn2` applies fd operations to a private child env before spawn

`proc_spawn2` first forks the current environment, then runs all requested fd operations against `child_env`, and only after that builds/spawns the new process:

- `lib/wasix/src/syscalls/wasix/proc_spawn2.rs`
  - `ctx.data().fork()` creates `child_env` before fd operations.
  - `for fd_op in fd_ops { apply_fd_op(&mut child_env, ...) }` mutates only that child env.
  - Process creation happens later, after all fd ops finish.

Relevant code:

- `proc_spawn2` forks: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:120`
- fd ops loop: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:151`
- process spawn begins after that: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:156`

### 2. `fork()` clones `WasiState`, and `WasiFs::fork()` clones `fd_map`

The child env does not share the parent `fd_map` lock or `FdList` object:

- `WasiEnv::fork()` creates `let state = Arc::new(self.state.fork());`
  - `lib/wasix/src/state/env.rs:261`
- `WasiState::fork()` uses `fs: self.fs.fork()`
  - `lib/wasix/src/state/mod.rs:255`
- `WasiFs::fork()` constructs a new `RwLock<FdList>` from `self.fd_map.read().unwrap().clone()`
  - `lib/wasix/src/fs/mod.rs:638`
  - specifically `fd_map: RwLock::new(self.fd_map.read().unwrap().clone())` at `lib/wasix/src/fs/mod.rs:641`

So the suspect race would require some other code path to access the same child-local `fd_map` concurrently before spawn. I did not find such a path in `proc_spawn2`.

### 3. `apply_fd_op(Dup2)` is synchronous; there is no await/yield between close and insert

The critical path is:

1. optionally read `target_fd`,
2. optionally `close_fd(op.fd)`,
3. take `fd_map.write()`,
4. look up `op.src_fd`,
5. `insert(true, op.fd, new_fd_entry)`.

Code:

- target read: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:211`
- close target: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:222-224`
- take write lock: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:226`
- source lookup: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:227`
- ignored exclusive insert: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:245-246`

`close_fd()` itself is also synchronous and only mutates the map under its own write lock:

- `lib/wasix/src/fs/mod.rs:2294-2312`

There is no `await`, `__asyncify_light`, or callback in this Dup2 branch between the close and the insert. With a private child `fd_map`, there is no visible allocator that can interleave here.

### 4. Why the ignored `insert(true, ...)` result does not appear to create corruption here

`FdList::insert(true, idx, fd)` returns `false` only if slot `idx` is already occupied:

- `lib/wasix/src/fs/fd_list.rs:140-173`

For the suspected corruption to occur, something would need to refill `op.fd` after `close_fd(op.fd)` and before `insert(true, op.fd, ...)`. In this function, that would require concurrent access to the same child-local `FdList`, which the surrounding spawn flow does not provide.

The only obvious non-racy way for `insert(true, op.fd, ...)` to fail is the self-dup case `op.src_fd == op.fd`:

- the code intentionally skips `close_fd(op.fd)` for self-dup at `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:222`
- it still calls `insert(true, op.fd, ...)` at `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:246`
- because the slot is still occupied, `insert()` returns `false`
- the result is ignored, so the original fd entry remains unchanged

That is sloppy, but it is effectively a no-op, which matches expected `dup2(fd, fd)` behavior better than the constructed replacement would have.

## Why this does not support the original corruption theory

The original suspicion was:

1. close target fd,
2. some other operation allocates the freed number,
3. exclusive insert fails,
4. return value is ignored,
5. wrong fd remains installed.

I do not see step 2 as reachable inside `proc_spawn2`:

- the map being mutated is a cloned child map, not the parent map,
- `apply_fd_op` runs before the child is started,
- the Dup2 branch contains no yield points,
- no other code in `proc_spawn2` is handed the child env concurrently.

Because the suspect depends on a missing concurrent allocator into the same child-local map, this specific fd-corruption theory is not substantiated.

## Separate issue worth noting, but not this suspect

There is a separate correctness bug candidate in the current Dup2 ordering:

- target is closed before validating that `op.src_fd` exists
  - close path: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:222-224`
  - source validation only later: `lib/wasix/src/syscalls/wasix/proc_spawn2.rs:227`

If `op.fd` exists, `op.src_fd != op.fd`, and `op.src_fd` is invalid, `apply_fd_op` will close the target and then return `Errno::Badf`.

That differs from the safer pattern in `fd_renumber_internal`, which validates the source before mutating the target and keeps the remove/insert under one write lock:

- source validation first: `lib/wasix/src/syscalls/wasi/fd_renumber.rs:71-75`
- target removal + insert under same lock: `lib/wasix/src/syscalls/wasi/fd_renumber.rs:69-107`

This is a real semantic discrepancy worth follow-up, but it is not evidence that suspect 7 causes fd-slot corruption through a close-before-lock race.

## Bottom line

Mark this suspect INVALID.

`proc_spawn2` Dup2 should probably still be refactored to match `fd_renumber_internal` for clarity and error handling, but I did not find a real corruption path from "close target before lock, another allocator grabs the slot, insert fails silently" in the current spawn architecture.
