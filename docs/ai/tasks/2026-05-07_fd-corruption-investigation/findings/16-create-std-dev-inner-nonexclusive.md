# Finding 16: `create_std_dev_inner` overwrites fd 0/1/2 non-exclusively

Status: INVALID

## Suspect

`WasiFs::create_std_dev_inner()` inserts the stdio fd with `exclusive = false`:

- `lib/wasix/src/fs/mod.rs:2205`
- `lib/wasix/src/fs/fd_list.rs:140`

That means a pre-existing entry at fd `0`, `1`, or `2` would be silently replaced instead of causing an error.

## What the code does

`create_std_dev_inner()` builds a new stdio inode/handle and then does:

```rust
self.fd_map.write().unwrap().insert(false, raw_fd, Fd { ... });
```

`FdList::insert(false, idx, fd)` explicitly replaces any existing entry at that slot:

```rust
if let Some(ref prev_fd) = self.fds[idx] {
    if exclusive {
        return false;
    } else {
        prev_fd.inode.drop_one_handle();
    }
}
```

So the low-level suspicion is real: if this helper were called against a live fd table containing active stdio entries, it would overwrite them.

## Call-site audit

I found only two effective call paths into `create_std_dev_inner()`:

1. Initial filesystem construction
   - `lib/wasix/src/fs/mod.rs:763`
   - `lib/wasix/src/fs/mod.rs:797`
   - `lib/wasix/src/fs/mod.rs:798`
   - `lib/wasix/src/fs/mod.rs:799`

   `WasiFs::new_init()` constructs a brand new `WasiFs` with:

   - `fd_map: RwLock::new(FdList::new())`

   and then immediately calls `create_stdin()`, `create_stdout()`, and `create_stderr()`. In this path the fd table is fresh and empty, so non-exclusive insertion cannot replace a live descriptor.

2. Environment reinitialization for reuse
   - `lib/wasix/src/state/env.rs:306`
   - `lib/wasix/src/state/env.rs:314`
   - `lib/wasix/src/state/env.rs:321`
   - `lib/wasix/src/state/env.rs:322`
   - `lib/wasix/src/state/env.rs:323`

   `WasiEnv::reinit()` is documented as:

   - `Re-initializes this environment so that it can be executed again`

   Before recreating stdio, it does:

   ```rust
   if let Ok(mut map) = self.state.fs.fd_map.write() {
       map.clear();
   }
   self.state.fs.preopen_fds.write().unwrap().clear();
   *self.state.fs.current_dir.lock().unwrap() = "/".to_string();
   ```

   Then it recreates stdin/stdout/stderr and root/preopens. So this path also intentionally starts from an empty fd table.

No other callers of `create_stdin()`, `create_stdout()`, `create_stderr()`, or `create_std_dev_inner()` were found in `lib/wasix/src`.

## Is `reinit()` a live-process mutation?

I checked the only `reinit()` caller:

- `lib/wasix/src/runners/dcgi/factory.rs:67`

This is part of DCGI instance recycling, not an in-place mutation of a still-running process.

Relevant evidence:

- `lib/wasix/src/runners/dcgi/callbacks.rs:40` recycles the environment after request handling.
- `lib/wasix/src/runners/dcgi/callbacks.rs:61` only then releases the instance back to the factory.
- `lib/wasix/src/runners/wcgi/handler.rs:89`
- `lib/wasix/src/runners/wcgi/handler.rs:100`
- `lib/wasix/src/runners/wcgi/handler.rs:101`
- `lib/wasix/src/runners/wcgi/handler.rs:102`

The handler explicitly delays token release until after recycling:

```rust
// We release the token after we recycle the environment
// so that race conditions (such as reusing instances) are
// avoided
drop(token);
```

The factory also serializes reuse behind a mutex:

- `lib/wasix/src/runners/dcgi/factory.rs:24`
- `lib/wasix/src/runners/dcgi/factory.rs:32`
- `lib/wasix/src/runners/dcgi/factory.rs:41`

`release()` stores one recyclable instance; `acquire()` takes it out under the same lock and only then calls `env.reinit()`.

I found no evidence that `reinit()` can run concurrently with guest code still using the old fd table.

## Conclusion

This suspect is **not** an independently real fd-corruption bug in the current codebase.

The helper is non-exclusive, but every current call site invokes it only when:

- building a brand new `WasiFs`, or
- fully resetting the fd table before rerunning a recycled environment.

So there is no demonstrated path where `create_std_dev_inner()` silently replaces fd `0/1/2` in a live process and redirects later writes.

## Why this is INVALID

The bad behavior requires a separate bug: some unexpected caller would need to invoke stdio recreation against a non-empty live fd table, or `reinit()` would need to be reachable concurrently with an active process despite the current reuse synchronization. I found no such path.

As written today, this is a hypothetical footgun / maintainability hazard, not a proven corruption mechanism.
