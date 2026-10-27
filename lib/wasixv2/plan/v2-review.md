# WASIX v2 senior review — `origin/feat/wasixv2` @ bdcdb958bef + pending branches

Reviewed (read-only, worktree `/home/theduke/dev/github.com/wasmerio/wasmer-review`, detached at
bdcdb958bef; nothing built):

- base: `lib/wasixv2/{exec,kernel,syscalls,wait,vfs,vfs-*,vnet,vnet-host,wasix1,api,host}`, `lib/cli/src/commands/run/*`
- `origin/wasixv2/bin-commands` (8 commits: package commands as files, exec `Loader`, `kernel_execve`, binfmt_script, `run_file`)
- `origin/wasixv2/v1-sigterm` (2 commits: `memory.atomic.wait/notify` on the kernel futexes)
- `origin/wasixv2/userland-fixes` (4 commits + submodule bump) and libc2 `origin/userland-fixes` (vfork function)

Method: one deep pass per area (exec by me; kernel process/signals, futex/mm/ipc/timers,
fs/vfs/poll, net, wasix1, facade/CLI by delegated reviewers), every high finding re-verified by me
against the code before listing. Linux reference: `/home/theduke/dev/github.com/torvalds/linux`.
Excluded: dynamic linking, closures, libffi, reflection.

Severity scale: **critical** = sandbox/data-corruption/host-panic reachable by any guest with
machine-wide effect; **high** = wrong results, host panic, or hang in common real-program paths;
**medium** = Linux divergence or leak with a plausible trigger; **low** = divergence with rare
trigger, perf polish, docs. No finding reached *critical* as defined; the closest are H1, H2, H4.

---

## 1. Architecture assessment

The design is sound and the code mostly honours it. The layering holds: the kernel is engine-free
and v1-free (every v1 import is a shim over `Syscalls`; the only v1-flavoured thing outside
`wasix1` is `set_wasix1_command_defaults` plumbing in the facade on bin-commands, see M-CLI3).
`wait`/`WaitQueue`, the three-level descriptor split, the VFS/driver split, the socket layer over
`vnet`, and the return-to-guest `Control` interface are clean, well-documented and close to their
Linux counterparts (large parts of fd table, pipe, poll/select/epoll, namei, port tables, tcp_poll,
scm_detach_fds, unix release rules, futex wait ordering, robust lists, SysV IPC, posix timers were
checked and found correct). Error precedence is generally Linux's.

Where it falls short:

1. **exec is committed before the point of no return is actually reached.** `exec_commit` swaps
   creds, fd table, args and mm immediately after `de_thread` *queues* SIGKILL for the siblings;
   the wait for them is in the exec layer, after the fact. Format recognition (`\0asm` / `#!`) is
   the kernel's, so an image no ABI serves is discovered after commit and becomes SIGSEGV instead
   of ENOEXEC. The `Loader` on bin-commands is a compile hook, not the binfmt-like registry the
   design describes. (H3, H6, WP1, WP2.)
2. **Hot paths allocate.** Every guest copy boxes a future and takes the store lock
   (`exec/memory.rs`), every `read`/`write` zero-allocates up to 64 KiB (`fs/io.rs`), every futex
   wait allocates ~6 times and `dequeue` scans every key under one global lock, every instance
   (thread spawn, exec) defines ~334 host functions whether or not the module imports them,
   and v1 shims issue extra syscalls per call (`getpid`, `FIONREAD`). Each is small; together they
   will dominate syscall-heavy workloads (shell scripts, Python imports, pthread-heavy servers).
   (WP3, WP4, WP8, M-X2, M-X5.)
3. **Robustness under host panics.** `catch_panic` turns a panic into SIGSEGV for the process, but
   311 `lock().unwrap()` sites on std mutexes mean a panic under any kernel lock poisons it and
   every other guest thread then panics too — one bad syscall degrades the whole machine. (M-X1.)
4. **The host bridge is thinner than the socket layer.** Outbound UDP has no backpressure path,
   TCP options set before connect/accept never reach the host socket, and a published connection
   is accepted by the host before the guest has room. (H7, H8, M-NET1.)
5. **Job control and controlling terminals are absent**, which the CLI's Ctrl-C handling works
   around rather than solves. (Gaps §5.)

The three pending branches are coherent and should merge; each has a short list of fixes below
(bin-commands: WP2 + process items; v1-sigterm: polish only; userland-fixes: correct, one
doc/libc nit).

---

## 2. Findings, ranked

Tags: **[base]** = feat/wasixv2, **[bin]** = bin-commands, **[sig]** = v1-sigterm,
**[usr]** = userland-fixes, **[libc2]** = libc2 userland-fixes. Paths are under
`lib/wasixv2/` unless noted.

### HIGH

**H1 — `RESOLVE_IN_ROOT` with an absolute path resolves inside the cwd, not `dirfd` [base]**
`kernel/src/fs/mod.rs:143` returns `env.fs.cwd` for any path starting with `/`, before looking at
`dfd`; `vfs/src/namei.rs:115-131` then sets `root = start` (= cwd) for `in_root`. So
`openat2(jailfd, "/etc/passwd", {resolve: RESOLVE_IN_ROOT})` opens `<cwd>/etc/passwd`, and an
invalid `jailfd` is never reported (EBADF/ENOTDIR). Linux `path_init`: `LOOKUP_IN_ROOT` strips the
leading slashes and starts at `dfd`. This is a sandboxing primitive giving a wrong answer.
Fix: in `Fs::start`/`do_open`, when `in_root` is set treat the path as relative (strip leading
`/`) and take the start from `dfd`, so `fd_path(dfd)?` runs.

**H2 — `mremap(MREMAP_FIXED)` onto a range overlapping the source copies zeroes [base]**
`kernel/src/mm.rs:637-645`: `map_fixed(new_addr, …)` runs before any overlap check; `map_fixed`
→ `unmap_range` + `take_pages` zeroes reused pages (`mm.rs:415-424`, `569-575`), then
`copy(addr64, target, keep)` copies the zeroed bytes. Linux `mm/mremap.c` (`vrm_overlaps`) returns
EINVAL. Fix: `if new_addr < addr+old_len && addr < new_addr+new_len { EINVAL }` before `map_fixed`.

**H3 — exec commits creds/fd table/args/mm before the other threads are dead [base, bin]**
`kernel/src/process.rs:657-732`: `de_thread` (`thread.rs:398-433`) only queues `Exit(0)` and
SIGKILL for the siblings and returns; `exec_commit` then unshares and closes cloexec fds, installs
the new credentials (`:711-719`), replaces args/exe/comm/mm. The wait for the siblings happens in
`exec/src/process.rs:199` (`exec_wait`) *after* the old image unwound. Linux `fs/exec.c de_thread`
blocks until `notify_count` reaches 0 before `begin_new_exec` reaches `commit_creds`/`exec_mmap`/
`do_close_on_exec`. Scenario: an old-image thread that reaches a syscall before its preemption
budget runs out executes it with the new image's credentials and fd table (a setuid image's euid
0, or EBADF on cloexec fds it was using). Fix: split `exec_commit`: after `de_thread`, await the
"alone" predicate `exec_wait` uses (on `process.group.released`) inside `sys_execveat`, then
commit; when the caller is not the leader, the commit half must run from the leader's side
(stash `Binary`+args next to the image and commit in `exec_wait`). Also return EAGAIN when the
group is already exiting (M-P2).

**H4 — Host panic: `signal_return` after `de_thread` took over a non-leader's signal state [base]**
`thread.rs:414-417` `take_over(task, leader)` → `signal.rs:398-408` *removes* `from`'s
`TaskState`. If the exec'ing thread was inside a signal handler (async-signal-safe `execve` from a
handler is legal), the `Exit(0)` unwind passes through `deliver_signal` and `exec/src/lib.rs:689-691`
calls `signal_return(task, None)`; `signal.rs:1027-1047` finds the task in the task table (not yet
released) and `signals.with(..)` → `GroupState::task()` → `signal.rs:629-633`
`expect("signal state of an unknown task")` panics; `catch_panic` turns it into SIGSEGV for the
process and (M-X1) may poison `Signals.state`. Linux `de_thread` keeps the task's own sighand
entry and swaps pids. Fix: `TaskSignals::with` / `signal_return` tolerate a taken-over task
(`tasks.get_mut` → return), or keep `from`'s entry until `forget_task` and copy it onto the leader.

**H5 — wasix1 scratch window: a guest pointer ≥ `SCRATCH_BASE` panics the host; writes there allocate 16 MiB and vanish [base]**
`wasix1/src/memory.rs:158-173`: for a read whose range lies in the window but beyond what was
allocated, `held = end.min(scratch.len()).max(start)` with `start > scratch.len()` gives
`&scratch[start..held]` → index panic (e.g. `fd_write(1, iovs=0xFF001000, …)` with an empty
scratch). The process dies of a host panic + SIGSEGV instead of `MEMVIOLATION`/`EFAULT`. The
`write` arm (`:175-190`) `resize`s the scratch up to 16 MiB and discards the data: the guest sees
success for a write into nowhere and can make every call allocate+zero 16 MiB. Fix: fault when
`start >= scratch.len()` (zero-fill only the tail of a range that starts inside an allocation);
never resize on write; longer term clamp v1 instances' memory maximum below `SCRATCH_BASE`
(the DESIGN claim "a v1 guest's memory cannot grow into the window" holds only for kernel-driven
growth, `memory.rs:201-209`; libc's `sbrk` uses `memory.grow`).

**H6 — Format is decided by the kernel before any loader is consulted: unservable images SIGSEGV instead of ENOEXEC [bin]**
`kernel/src/process.rs` (bin-commands, `exec_binprm` loop ~`:610-637`) accepts any
`bytes.starts_with(WASM_MAGIC)` and commits; ABI selection happens at instantiate
(`exec/src/abi.rs:199-201` `require` → `NoAbi`) and compile in `exec/src/process.rs:202-233`, where
every error is `Ended::Fault { SIGSEGV }` with a host report. A guest shell running a plain-wasm
atom, a `wasix_64v1` module or a truncated `.wasm` dies with 139; `execvp`'s ENOEXEC → `/bin/sh`
fallback and `command -v` probing never work. Linux `search_binary_handler` returns ENOEXEC before
`begin_new_exec`. Fix: give the registry a cheap pre-commit probe
(`Loader::probe(&[u8]) -> Result<(), Errno>`, built on the existing `is_wasixv2_module` /
`is_wasix1_module` import-section parsers and `V1Flavor::unsupported_reason`), called where the
kernel now checks `WASM_MAGIC`; `LoadError` gets a `Format(Errno)` variant. This is exactly
`search_binary_handler`; the kernel still owns path and permissions.

**H7 — Blocking UDP send to the host can hang forever; `poll` lies `OUT` [base]**
`vnet-host/src/reactor.rs:834-838` registers datagram sockets with `Interest::READABLE` only;
`HostDatagram::poll` (`:736`) unconditionally reports `OUT|WRNORM`; `UdpSocket::send`
(`vnet/src/udp.rs:378-380`) calls `send_to` directly on the guest thread. On host `EAGAIN` (full
`SO_SNDBUF`, common for fast senders) `Blocking::wait` (`kernel/src/socket.rs:112-135`) sleeps on
the socket queue for `WRITABLE` and nothing ever wakes it; non-blocking senders spin. Linux
`udp_sendmsg` → `sock_alloc_send_pskb` sleeps and `sock_wfree` wakes. Fix: a reactor-pumped tx
queue for datagrams (as streams have), or register `WRITABLE`, track `host_writable` and report
`OUT` from it.

**H8 — Socket options set before `connect`/`accept` never reach the host socket [base]**
`vnet/src/tcp.rs:849-853` forwards `set_option` to the backend stream only when a
`Transport::Remote` exists; `connect` (`:665-668`) creates the stream without replaying `g.opts`;
`new_child` for published connections copies `opts.inherit()` but never applies them (`:378-404`,
`:424`). `HostStream::set_option` (`reactor.rs:576-600`) is the only path to
`set_tcp_nodelay`/`set_keepalive`/linger. Consequence: the idiomatic `setsockopt(TCP_NODELAY)`
before `connect()` leaves Nagle on; `SO_LINGER{on, N>0}` set before connect is treated as no-linger.
Linux applies `sk` fields regardless of state and `inet_csk_clone_lock` inherits them. Fix: replay
`g.opts` (nodelay, keepalive*, linger, buffer sizes) onto a freshly created/accepted `BackendStream`.

**H9 — SCM_RIGHTS cycles leak and exhaust one kernel-wide in-flight budget [base]**
`kernel/src/unix.rs:120,151-181,1328-1331`: `Names.inflight` is a single counter for the whole
kernel, compared against the *sender's* `RLIMIT_NOFILE`, and nothing ever collects cycles.
`socketpair(a,b); sendmsg(a, SCM_RIGHTS=[b]); close(a); close(b)` leaks `b` forever and the
counter never decrements; ~1024 such leaks by one unprivileged process make every `SCM_RIGHTS`
send by every process fail with `ETOOMANYREFS`. Linux: per-user `unix_inflight`, `unix_gc`,
`CAP_SYS_RESOURCE` bypass. Fix: per-uid counter now; a mark/sweep `unix_gc` over `Msg.rights` of
Unix sockets as a follow-up.

**H10 — Every `execve` pays ~4 full passes and ~3 copies of the image even on a cache hit; `wasmer run file.wasm` compiles twice [bin, base]**
bin-commands `kernel/src/process.rs:676-684` copies the whole file out of the package
`OwnedBuffer` (`vec![0; attr.size]` before any format check — a 4 GiB file allocates 4 GiB);
`lib/cli/src/commands/run/machine.rs` `CachedLoader::load` (~`:174-187`) then runs
`detect_features_from_wasm` (full parse), `instrument(image).into_owned()` (full rewrite, second
copy) and hashes the *instrumented* bytes before the cache lookup. A shell loop spawning `python`
50× does 50× (copy 30 MB + parse + rewrite + hash). Base: `run/target.rs:104-112` compiles the
uninstrumented bytes for `resolve_target` and `run/mod.rs:166-174` compiles again with preemption
points; the first module is only used for an import check that `is_wasixv2_module`/
`is_wasix1_module` already answer from bytes. Fix: hand the loader an identity (mount+inode+size,
or the package buffer pointer) and the raw bytes as `Arc<[u8]>`/`OwnedBuffer`; key the cache on
identity or the raw-bytes hash and instrument only on a miss; probe with the first 256 bytes
(`BINPRM_BUF_SIZE`); decide "runs on machine" from bytes and compile once.

**H11 — wasix1: no DNS without a guest-visible `/etc/resolv.conf` [base]**
`wasix1/src/calls/resolve.rs:80-82` falls back to `127.0.0.1`, then `lookup` (`:491-504`) tries
`attempts × nameservers` with 5 s each → `TIMEDOUT`. Legacy v1 resolved through the host, so every
networked v1 program (pip, npm, curl, php) had DNS; here `getaddrinfo` fails or hangs ~10 s per name
unless the embedder mounts a resolv.conf naming a server reachable from the guest netns. Fix:
`wasmer run` writes `/etc/resolv.conf` from `vnet-host::host_nameservers` (already exists) by
default; make the loopback fallback fail fast. (The in-process DNS server stays an open question.)

**H12 — wasix1: after `pthread_create`, v1 threads keep every signal blocked forever [base]**
`wasix1/src/calls/signal.rs:194-222` implements `callback_signal("__wasm_signal_blocked")` as
"set the calling thread's mask to all-blocked". wasix-libc's `pthread_create` calls
`__block_app_sigs` unconditionally and the matching `__restore_sigs` is compiled out
(`libc-top-half/musl/src/thread/pthread_create.c:534,614`). Result: the spawning thread and the
child (which inherits the mask at `clone`) have SIGINT/SIGTERM/SIGALRM pending forever; a
multithreaded Python/Node/Rust port becomes immune to Ctrl-C/`kill -TERM` and `alarm()` handlers
never fire. Legacy dropped such signals instead (not better, but programs stayed killable). Fix:
the real fix is in wasix-libc; in the shim, do not install a kernel mask nothing clears — map the
"blocked" export to withholding *handler* delivery (default actions still apply), and document.

### MEDIUM

**M-P1 — ABBA deadlock in `may_signal` [base]** `kernel/src/signal.rs:1266-1267` holds the
sender's `ident` guard while locking the target's, with no outer lock; two processes of different
uids sending each other `SIGCONT` concurrently deadlock. Fix: copy both sids into locals first.

**M-P2 — exec on a dying group proceeds and leaks the image [base, bin]** No
`signal_group_exit` check in `sys_execveat`/`exec_commit`; a SIGKILL during `open_exec` still runs
the commit and `hand_image` queues `Exec` behind the `Exit`; `tree.images` is only drained by
`take_image`, so the `Vec<u8>` leaks per occurrence. Linux `de_thread`: EAGAIN. Fix: check
`process.group.exiting()` at the top of `exec_commit`; drop `images[task]` in `forget_task`.

**M-P3 — Child task leaks when the parent's exit is queued ahead of `SwitchTask`/`Fork` [base]**
`kernel/src/work.rs:31-56` aborts only `StartThread` when an exit is queued; `SwitchTask`/`Fork`
queue behind the `Exit`, `exit_status` (`exec/src/process.rs:296-304`) drains them, and the child
stays in `State.tasks` with `live = 1` and a tree node, never released or reaped. Fix: return the
child from `push` for abort as for `StartThread`; `queue()` aborts a process child (forget task,
remove tree node and `vforks` entry).

**M-P4 — Realtime signal queue is unbounded [base]** `signal.rs:111-120` never fails;
`RLIMIT_SIGPENDING` exists (`resource.rs:104`) but only posix timers consult it. Linux
`__sigqueue_alloc` → EAGAIN for `sig >= SIGRTMIN`. Fix: per-user queued-signal counter in
`GroupState::prepare`/`send`.

**M-P5 — argv/envp/shebang lossily converted to UTF-8 [base, bin]** `process.rs:1070`,
bin-commands `lossy()`; `Process.args/env: Vec<String>`. `execve("ls", ["ls", "caf\xe9"])` →
ENOENT in the child; `/proc/<pid>/cmdline` too. `startup.rs` writes bytes anyway. Fix:
`Vec<Vec<u8>>` end to end.

**M-P6 — sigaltstack "on stack" is a frame counter, not a stack-pointer test [base]**
`signal.rs:211-213,969-978,1428-1441`. A handler that switches coroutines away
(`context-switch`, a design feature for greenlet) leaves the frame pushed, so every later
`SA_ONSTACK` signal lands on the normal stack and `sigaltstack()` returns EPERM until that frame
returns; `SS_AUTODISARM` reports `SS_ONSTACK`+EPERM inside the handler (Linux: `SS_DISABLE`,
re-arm allowed). Fix: pass `__stack_pointer` from exec to `take_signal`/`sigaltstack` and compare
with `[sp, sp+size)` (`on_sig_stack`); keep the frame stack only for `SS_AUTODISARM`.

**M-P7 — `PR_SET_PDEATHSIG` stored but never delivered [base]** `ident.rs:561-566`; Linux
`forget_original_parent` sends it per reparented child. Fix: in `process_exited`'s orphan loop
(`process.rs:266-273`); clear it on cred change as `commit_creds` does.

**M-P8 — `kernel_execve` pops whatever action is first and `debug_assert`s it is the `Exec` [bin]**
bin-commands `process.rs kernel_execve`: in release an `Exit` queued by the embedder before the
image starts is silently discarded, and `expect("a committed execve leaves its image")` can panic.
Fix: remove the `Exec` specifically (`retain`); EINTR otherwise.

**M-P9 — vfork parent not killable while the child runs, and reapable while its instance still runs [base]**
Linux `wait_for_vfork_done` is `TASK_KILLABLE`. Here `do_group_exit(parent)` → `process_exited`
runs now (fds close, zombie, children reparented, grandparent may reap) while the child still runs
in the parent's instance; on `VforkReturn` the instance resumes guest code of a reaped process.
Fix (minimum): defer `process_exited` for a parent with a live `tree.vforks` entry; document.

**M-F1 — `futex_wait` is not cancellation-safe; a dropped future leaks its `Entry` and absorbs a later `FUTEX_WAKE` [base, sig inherits]**
`kernel/src/futex.rs:395-406` enqueues with no drop guard (every exit path dequeues by hand;
same for `PiWaiter` at `:576-578` and requeue at `:700`), unlike `wait.rs:680-696` and
`sem.rs:222-227`. Today syscall futures are only dropped by `catch_panic` (M-X1), so the blast
radius is the dying process's `mm`; the design promises cancellation-safe waits. Fix: a guard
`(key, Arc<Entry>)` whose `Drop` dequeues; store the key in `Entry` (also fixes M-F3).

**M-F2 — No `ERESTARTNOINTR`: PI waits can fail with EINTR [base]** `futex.rs:584-585,755-756`
return `SysErr::Restart`; Linux `pi.c`/`requeue.c` return `-ERESTARTNOINTR` (always restarted,
even after a handler). Fix: `SysErr::RestartNoIntr` + unconditional rerun in `complete`
(`exec/src/lib.rs:700-708`). Would also make `memory.atomic.wait` spec-exact (M-SIG1).

**M-F3 — `Futexes::dequeue` is O(#keys × queue length) under one global lock; ~6 allocations per wait [base]**
`futex.rs:99-118` iterates every key's `VecDeque` because `Entry` (`:46-54`) carries no key; runs
on every timeout/EAGAIN/signal/exit path. Each wait allocates `Arc<Entry>`, `WaitQueue`, `Waiter`,
2-3 `Arc<KeyedEntry>` *per `sleep` call* (`:402,405`, `wait.rs:516-518`) plus the boxed sleep.
Fix: key in `Entry`, `queues.get_mut(&key)`, per-key buckets, register on the interrupt queue once
per wait.

**M-F4 — `FUTEX_CMP_REQUEUE` compares `val3` outside the queue lock [base]** `futex.rs:294-299`
reads, then `requeue` locks at `:154`; Linux compares under both hb locks. Fix: compare inside
`requeue`.

**M-F5 — `exit_group`/kill never walk the robust list or clear `clear_child_tid` [base]**
`thread.rs:468-485` `exit_thread` only from `sys_exit` (`:231`); a vfork child that `exit_group`s
(or any killed thread) leaves its parent's robust mutexes locked and the tid unwritten. Linux runs
`futex_exit_release` in every `do_exit`. Fix: run `exit_thread` for the caller in `sys_exit_group`
and when a task is released after a group exit. Related: exec drops `t.thread` without
`futex_exec_release` (`process.rs:731`).

**M-F6 — Unchecked `u32` arithmetic on guest pointers [base]** `ipc/msg.rs:236,342`
`msgp.addr() + 4`, `futex.rs:882` `head + 4/8`: a guest passing `0xFFFFFFFC` panics with overflow
checks (test/dev builds) and reads address 0..3 in release. Fix: `checked_add` → `Fault`, as
`read_cstr` does (`syscalls/src/memory.rs:140`). Also `UserSliceMut::copy_out` only
`debug_assert`s `data.len() <= self.len` (`:179`): make it a real `Fault`.

**M-F7 — SysV segment / shared mapping sync cost [base]** `ipc/shm.rs:90-101` `Segment::read`
does a `HashMap` lookup per byte; `sync_segments` runs before *and* after `semop`/`msg*`/`mq_*`
and each `Mapping::sync` (`mm/shared.rs:257-279`) snapshots the whole mapping twice. 16 MiB
attachment ≈ 32 M hash lookups + 64 MiB copies per `semop`. Same shape for `sync_file_mappings`
on every `read`/`write` of a mapped file. Fix: page-wise copy in `read`; page-granular dirty
tracking.

**M-FS1 — SIGPIPE not raised when the last reader disappears mid-write [base]**
`kernel/src/pipe.rs:245-249,258` return `Ok(written)` on EPIPE after partial progress;
`fs/io.rs:251` `Err(_) if done > 0 => break` swallows a later EPIPE; `io.rs:303-313` signals only on
`Err(EPIPE)`. Linux `pipe_write` sends SIGPIPE whenever `!pipe->readers` and returns the partial
count. `yes | head`-style writers die one write late. Fix: report "readers gone" out of band from
`write_bytes` so `write_buf`/`writev`/`sendfile` raise SIGPIPE on a positive return.

**M-FS2 — every `read`/`write`/`sendfile`/`copy_file_range` zero-allocates up to 64 KiB [base]**
`fs/io.rs:180,237,505,592` `vec![0; total.min(IO_CHUNK)]`. Fix: per-task reusable scratch, or
`MaybeUninit` + copy-in without zeroing.

**M-FS3 — stream reads capped at one 64 KiB step [base; usr lifts it for devices only]**
`fs/io.rs:196`: `read(fd, buf, 1<<20)` on a pipe with 1 MiB buffered or a TCP socket returns
≤ 64 KiB; Linux returns what is available up to `count`. Fix: let objects read into the full
guest-sized buffer (drain-what-is-there for streams).

**M-FS4 — `fcntl(F_SETLK)` close race leaks a POSIX lock [base]** `fs/lock.rs:153`; Linux
`fcntl_setlk` re-checks `files_lookup_fd_rcu(fd) != filp` after acquisition and releases. Fix:
after `lock_wait`, re-check `files.get(fd)` is `ptr_eq` to `file`, else `release_owner`.

**M-FS5 — epoll loses events when the copy-out faults after disarming [base]**
`kernel/src/epoll.rs:690-694`: ONESHOT cleared / ET items dropped from the ready list before the
per-event `write_value`; Linux `ep_send_events` re-queues un-copied items. Fix: build the guest
buffer first (one bulk write), re-queue on failure.

**M-FS6 — `/proc/self/fd/N` of a pipe blocks for a partner on base [base; fixed on usr]**
Base `fs/fifo.rs:38-53` waits for a writer/reader on pipefs inodes too (`cat <(cmd)` hangs after
the writer exits). The userland-fixes change (`is_pipe = sb.magic == PIPEFS_MAGIC`, three guarded
sites) matches Linux `fifo_open`'s `is_pipe` exactly and is covered by a test. **Merge.**

**M-NET1 — Published connection arriving while the guest accept queue is full is accepted by the host, then closed [base]**
`vnet/src/tcp.rs:418-430` `incoming_remote` returns `false` when full after `accept_all`
(`reactor.rs:618-624`) completed the host handshake → `HostStream::drop` → FIN; the remote sees
`connect()` succeed then EOF, where Linux drops the SYN (`tcp_v4_syn_recv_sock` →
`sk_acceptq_is_full`). Host listener backlog is unrelated to `listen(backlog)`. Fix: do not call
host `accept()` unless the guest queue has room (track `host_readable`, re-run `accept_all` from
`Listener::pop`), and set the host backlog from the guest's.

**M-NET2 — Reactor busy-loops on a persistent datagram receive error [base]**
`reactor.rs:692-715` only exits on `WouldBlock`; any other error is recorded and the loop
continues, starving every other host socket on the single reactor thread. Fix: `break` after
recording.

**M-NET3 — Control messages on IP sends are silently ignored [base]** `tcp.rs:731-747`,
`udp.rs:314-383` never read `msg.control`; Linux `__sock_cmsg_send`/`ip_cmsg_send` reject unknown
or short cmsgs with EINVAL and apply `IP_TTL`/`IP_TOS`/`IP_PKTINFO`. Fix: validate known
types/lengths, EINVAL otherwise.

**M-NET4 — `SO_REUSEPORT` same-uid rule unenforceable: `BindSpec.uid` is always 0 [base]**
`tcp.rs:252-261`, `udp.rs:91-100`, `ports.rs:129`; `Socket::bind` (`api.rs:171`) carries only
`privileged`. Fix: pass the owner uid through `bind`/`listen`.

**M-NET5 — Unix `getsockopt` asymmetric with `setsockopt`; `SO_PEERCRED` unconnected reports 65534 [base]**
`unix.rs:784` accepts every `SOL_SOCKET` name as a no-op, `unix.rs:1334-1358` returns
`ENOPROTOOPT` for `SO_LINGER`, `SO_RCVLOWAT`, `SO_OOBINLINE`, `SO_PRIORITY`, `SO_TIMESTAMP*`,
`SO_PEERGROUPS`, …; Linux `cred_to_ucred` reports `-1` unconnected. Fix: route through
`SockOpts::get` with a Unix flavour.

**M-NET6 — `tcp_disconnect` divergences [base]** `tcp.rs:679-695`: no `sk_err = ECONNRESET` on
self, and the auto-bound port is released (Linux keeps `inet_num`). Observable via `SO_ERROR` and
`getsockname`.

**M-W1 — wasix1 `ProcState::fork()` drops the fs/net compat state its doc says it copies [base]**
`wasix1/src/process.rs:27-38`: after `proc_fork` (asyncify) or vfork, the child's epoll side table,
`CONNECT/ACCEPT_TIMEOUT`s and replaced-preopen set are empty; `epoll_wait` in a forked worker
returns `EpollData { fd, ..ZERO }` (`calls/epoll.rs:126-137`) — prefork servers dispatch on
garbage. Fix: clone the tables (they are keyed by descriptor numbers the child shares).

**M-W2 — wasix1 preopens must land on fds 3/4/5 or libc finds no filesystem [base]**
`calls/startup.rs:101-127,213-233`: preopens open on the lowest free fds at the first v1 call;
wasix-libc scans from 3 and stops at the first `BADF` (`preopens.c:233-241`). Any inherited fd 3
(bash `3>file`, Node IPC, `posix_spawn` with pass_fds, a v2 parent passing a socket) → zero
preopens → every path call ENOENT. Legacy had the same limit. Fix direction: re-home inherited
fds ≥ 3 upward when a v1 image starts, or document in the compatibility promise.

**M-W3 — wasix1 syscall multiplication per call [base]** `shim.rs:62-65` `Frame::proc()` issues
`getpid` on every compat-state access; `fd_close` = 2×`getpid` + `close` (`fs.rs:41-51`,
`io.rs:46-59`); `poll_oneoff` does `FIONREAD`(+`statx`) per ready fd for an `nbytes` libc ignores
(`poll.rs:250-286,320`); `CompatMemory` is rebuilt with an empty `Vec` per import call
(`shim.rs:123`, `memory.rs:33-38`: `fd_readdir` zeroes 16 KiB, `getcwd` 4 KiB per call);
`inherited()`/`install()` issue ~180 `rt_sigaction` per process start (`signal.rs:125-190`);
`imports()` defines all ~250 v1 functions for every instance (`imports.rs:251-267`);
`Errno::from_linux` is a 130-branch `if` chain (`abi/errno.rs:19-22`). These also clutter the
trace/record layers' view of "a v1 program as v2 calls". Fix: cache pid per `Wasix1Instance` /
vfork map; reusable scratch per instance; report `nbytes = 0`; one sigaction pass; filter imports
by `module.imports()`; errno lookup table.

**M-W4 — wasix1 `tty_set` maps `line_feeds` onto `IGNCR` [base]** `calls/tty.rs:100-128`;
wasix-libc derives `line_feeds` from `c_lflag & IGNCR` (an iflag constant tested against lflag, i.e.
`ECHONL`); legacy ignored the field. Setting `ECHONL` now turns on `IGNCR` on the host tty. Fix:
no-op (legacy) or `ONLCR`; never `IGNCR`.

**M-W5 — wasix1 package command defaults live in the v1 shim and match by exact path string [bin]**
bin-commands `calls/proc.rs:306-316` looks up the candidate string in `Procs::with_commands`;
`api/src/lib.rs:312-315` registers `/bin/x` and `/usr/bin/x`. A v2 program `execve`'ing
`/bin/python`, or a v1 program reaching it via `./python`, a symlink or `cwd=/bin`, gets no
defaults; `Procs` (per-pid state) now carries machine configuration; `set_wasix1_command_defaults`
rebuilds the whole `Abis` and the facade keeps `loader` twice. `exec-name`/`main-args`/`env` are
properties of the image file, not of the calling ABI. Fix: attach them to the command file in the
loader/binfmt registry keyed by resolved identity; `Wasix1` holds `Arc<RwLock<Commands>>`.

**M-CLI1 — Ctrl-C goes to the initial process's pgrp and the 3-strike escape never fires for a guest that ignores SIGINT [base]**
`kernel/src/signal.rs:1305-1317` targets the initial pid's pgrp (a shell that `setpgid`s its
foreground job never sees `^C`); `machine.rs:658-665` resets `unanswered` whenever
`foreground_pending` is false and `signal.rs:559` discards an unblocked *ignored* signal at
generation, so `sh -c 'trap "" INT; while :; do :; done'` is uninterruptible despite the doc
comment. Fix: count a press as unanswered when the kernel reports it ignored/unconsumed; a
controlling terminal with `fg_pgrp` (gap §5).

**M-CLI2 — `Machine::run_file` duplicates the exec path with different failure semantics [bin]**
`api/src/lib.rs:476-505`: after `kernel.exec` committed, a loader error calls `abandon`
(`:637-643`: `fault(SIGKILL)` + `release`, skipping `exit_status`/`take_pending`) and returns
`RunError::Load`; the same event through `execve` is SIGSEGV/139 with a host report. Fix: let
`process::run` accept an initial `Ended::Exec(image)` so `run_file` = `kernel.exec` + the one loop.

**M-CLI3 — `Loader` abstraction is half there [bin]** `exec/src/loader.rs`: `LoadError =
Box<dyn Error>` cannot say ENOEXEC; no identity for caching (H10); `engine` is documented but the
only real implementation ignores it (`CachedLoader` makes its own engines, so the CLI's engine from
`mod.rs:116` and `runtime.engine()` at `machine.rs:457` are unused duplicates); docs in
`loader.rs:1-4,11-13,21-22` and DESIGN "A registry of further formats, like binfmt_misc" describe a
registry and recognition that do not exist. Fix with H6/H10: `Loader::load(Image { bytes,
identity })`, `probe`, `Format(Errno)`; seed `CachedLoader` with the CLI's engine.

**M-CLI4 — Host scripts usable only in narrow setups [bin]** `machine.rs:474-499` refuses a
script outside a `--volume`; `#!/usr/bin/env python3` needs `env`/`python3` in the guest but
`--use` is refused (`:574`) and nothing installs a package's commands for a script run;
`machine_env(None)` gives an empty environment (no `PATH`, `HOME`). Fix: auto-mount the script's
directory read-only at a synthetic path; implement `--use` via `install_commands` +
`mount_package_volume`; a minimal default env.

**M-CLI5 — Package mounting re-parses and re-boots [base, bin]** `machine.rs:717-730`
`Reader::open` re-mmaps/re-parses a V3 package whose parsed `OwnedReader` is already in hand;
`api/src/lib.rs:321-336` `push_root_lower` boots a new `Kernel` and remounts `/proc`,`/dev`,`/tmp`
per root layer (N volumes + commands layer + M host dirs = N+M+1 boots); `install_commands`
pushes a layer even when empty; `PackageFs::with_owner` validates every directory per mount. Fix:
downcast first; collect lowers and boot once; skip empty layers; cache validation per reader.

**M-CLI6 — One tokio `current_thread` runtime per guest thread; no machine shutdown [base]**
`host/src/tokio_host.rs:86-94`: each guest thread builds its own runtime (own epoll driver,
timer wheel, up to 512 blocking threads); `spawn_local` work dies with the thread. `Machine` has
no `shutdown`; the CLI calls `std::process::exit`, an embedder dropping a running `Machine` leaks
threads and the kernel. Fix: capture a `Handle` at construction and drive guest threads on a
`LocalSet` over `handle.block_on`; `Machine::shutdown()` = SIGKILL all + wait for releases.

**M-X1 — A host panic under any kernel lock poisons it and cascades to every guest thread [base]**
311 `lock().unwrap()` sites in `kernel/src`; `exec/src/lib.rs:320-331` `catch_panic` recovers the
panicking thread (SIGSEGV for its process, `host_panicked`), but every other thread that next
touches the poisoned mutex panics too and kills *its* process. One bad syscall (H4, H5, M-F6)
degrades the whole machine. Fix: `parking_lot` mutexes, or one `lock()` helper that
`unwrap_or_else(PoisonError::into_inner)`.

**M-X2 — Every guest-memory copy boxes a future and takes the store write lock [base]**
`exec/src/memory.rs:21-53`: `GuestMemory::read/write/size` each `Box::pin` and `env.write().await`;
a `readv` with 8 iovecs is 9+ allocations and lock acquisitions before any data moves, on top of
M-FS2. Fix: an unboxed fast path (sync `read_sync`/`write_sync` on `UserMemory` that the exec layer
can serve without awaiting), or batch copies per syscall.

**M-X3 — `copy_to_vec`/instantiation cost of threads and exec: ~334 host functions per instance [base]**
`exec/src/generated.rs` `imports()` defines every `wasixv2` import (334 `define`s) for every
instance — each thread spawn (`exec/src/thread.rs:147`) and each exec — whatever the module
imports (wasix1 adds ~250 more, M-W3). Fix: filter by `module.imports()`.

**M-X4 — `instrument()` silently runs an un-instrumentable module without preemption or atomic interception [base, sig]**
`exec/src/preempt.rs:35-45` logs at `debug` and returns the module unchanged when the reencoder
fails (any proposal `wasm_encoder`'s reencode does not handle); such a guest never takes signals
while busy and (sig) blocks the host thread in `memory.atomic.wait`. Fix: `warn!`, and surface it
to the embedder (`load` returns which pass applied).

**M-SIG1 — `memory.atomic.wait` returns 0 ("woken") when a non-restarting handler interrupts it [sig]**
`exec/src/atomics.rs:160-168` maps every errno but `EAGAIN`/`ETIMEDOUT` to 0. Documented and
deliberate (`atomics.rs:41-43`); the timed case is right (`return_to_guest` runs `restart_syscall`
to the original deadline). With M-F2's `RestartNoIntr` the untimed wait could be spec-exact
(always restart). Nit: a stale `FutexWait` restart block after the hook returned 0 is harmless but
could be cleared on the EINTR path. Otherwise the branch is sound: keying 32/64-bit waits on
`(mm, addr)` so `notify` and `FUTEX_WAKE` reach both, queue-then-compare ordering, OOB/unaligned
traps, `count == 0` → 0, start function left alone. **Merge.**

**M-USR1 — Saved asyncify stacks can accumulate per distinct `jmp_buf` address inside a long-lived frame [usr; pre-existing]**
`exec/src/asyncify.rs` (usr) keys saves by generation and drops them only when `dead_after` proves
the frame returned (sp above, or same slot+sp+len). A loop in one frame doing
`ctx = malloc(); setjmp(ctx->jb); …; free(ctx)` with varying addresses keeps one unwound-stack copy
per distinct address until the level returns. Not a regression (the slot-keyed map grew the same
way), the heuristic itself is sound and tested, and the bash `return_catch` case it fixes is real.
Fix direction: cap saves per level (LRU by generation), or let libc hint `jmp_buf` death. **Merge.**

**M-LIBC1 — libc2's new `vfork` function trades a link failure for a run-time ENOSYS in the default configuration, with no diagnostic [libc2]**
`src/process/wasm32/vfork.c:75-78` `pid_t (vfork)(void) { return _Fork(); }`; in the exnref sysroot
fork is ENOSYS, so `(vfork)()`, a function pointer or FFI now links and fails at run time reported as
`vfork`. `include/unistd.h:162-184` drops the `__unavailable__` attribute. Correct otherwise: does
not touch `points[]`/`depth`, `(vfork)` prevents expansion, feature-test visibility unchanged,
autoconf rationale holds. Stale comment at `vfork.c:90-91` still says there is no function. Fix:
keep the symbol, declare it `__attribute__((__deprecated__("vfork called as a function forks;
ENOSYS without asyncify — use the macro")))` (the macro path never references the declaration);
delete the stale comment; DESIGN decision row should mention the ENOSYS consequence. Merge order:
libc2 first and sysroot rebuilt, since the wasmer bump (`9a8c6df66b0`) adds `proc-vfork-autoconf`.

### LOW

- **L-P1** Group exit status when the leader exited first: `thread.rs:228-251` uses the last
  thread's status; Linux `wait_task_zombie` uses the leader's unless `SIGNAL_GROUP_EXIT`. [base]
- **L-P2** Leader's temporary mask (`sigsuspend`/`ppoll`) clobbers the exec'ing thread's mask after
  `take_over`: `exec_reset` (`signal.rs:329-345`) must clear `saved`/`notify`. [base]
- **L-P3** Fork/vfork child loses the alternate signal stack (`signal.rs:298-324`); Linux resets
  only for `CLONE_VM` without `CLONE_VFORK`. [base]
- **L-P4** `CLONE_PIDFD` silently ignored (pidfd never written; `process.rs:333-375`); return
  EINVAL until implemented. `clone3` ENOSYS is fine for musl. [base]
- **L-P5** `pidfd_send_signal` to a zombie → ESRCH (Linux succeeds), `info` ignored
  (`process.rs:989-999`). [base]
- **L-P6** `mm.reset()`/`mm_release` run from the first exiting thread (`thread.rs:275,316-318`);
  Linux at the last `mmput`. Bookkeeping only. [base]
- **L-P7** Global `Procs.changed` wakes every `waitid`/pidfd waiter on any exit; `groups()` takes
  the tree lock per task under the state lock on every `kill(pid)`; `group_signals` takes a write
  lock for a read; `Tree::children`/`reaper`/`setpgid`/`setsid` are full scans. Fine today; needs
  per-parent child lists before it grows. [base]
- **L-P8** Pids never reused, `TaskId(u32) as i32` wraps negative past 2^31; DESIGN promises reuse.
  [base]
- **L-F1** `ipcget` runs perm check before `more_checks` (`ipc/mod.rs:299-304`; Linux reverse):
  EACCES vs EINVAL. **L-F2** `msgctl(IPC_SET)` does not `expunge_all(-EAGAIN)` (`msg.rs:406-421`).
  **L-F3** `FUTEX_WAKE_OP` shift arg EINVAL vs Linux `oparg &= 31` (`futex.rs:985-990`).
  **L-F4** itimer/posix-timer interval zeroing diverge in opposite directions
  (`signal.rs:1535-1536`, `posix_timer.rs:348-351`); `real_timer_expired` re-arms while blocked.
  **L-F5** `WaitQueue::wake` contract permits dropping registrations on other queues under the
  lock (`queue.rs:41-47,203-226`) — ABBA if two entries ever do; forbid or defer. **L-F6**
  `TFD_TIMER_CANCEL_ON_SET` accepted and ignored; realtime deadlines converted once to monotonic.
  **L-F7** `mq_unlink` has no owner check (`mqueue.rs:426-434`). **L-F8** `semtimedop` error
  precedence (`sem.rs:369-383`). **L-F9** `sched_setaffinity` accepts short/unaligned `len`
  (`sched.rs:898-905`). **L-F10** `mremap` rejects `old_len == 0` for shared mappings and
  `MREMAP_DONTUNMAP` (`mm.rs:619-627`). **L-F11** DESIGN.md:143 says futexes are keyed by memory
  object so shared mappings work across processes; `futex.rs:40-43` keys by `(Mm::id, addr)` and
  DESIGN:165 says the opposite — fix the bullet (the sig patch repeats the wrong sentence). [base]
- **L-FS1** `write()` of >8 bytes to an eventfd succeeds (`eventfd.rs:47`; Linux EINVAL).
  **L-FS2** `flock()` on pipes/sockets is a silent no-op (`fs/lock.rs:43-46`). **L-FS3**
  `fallocate` confuses `O_APPEND` with `S_APPEND` (`fs/io.rs:726-730`). **L-FS4** `sendfile`
  demands a seekable `in_fd` even with `offset == NULL` and re-looks-up `out_fd` after the
  transfer for SIGPIPE (`fs/io.rs:494,546-549`). **L-FS5** `inotify_add_watch` rejects control-bit-
  only masks, `IN_EXCL_UNLINK` accepted but unhonoured (`fs/inotify.rs:34,404`); watches do not pin
  the inode (ino reuse on host dirs). **L-FS6** stale TODO at `poll.rs:61-63` (sigmask restore *is*
  implemented via `saved`). **L-FS7** `copy_file_range` lacks the EOVERFLOW check
  (`fs/io.rs:584-590`). **L-FS8** `F_GETFL` drops `O_DIRECTORY|O_NOFOLLOW`, `F_SETFL` accepts
  `O_DIRECT` on anything (`file.rs:97-99`). **L-FS9** unreachable arm `vfs/src/ops.rs:254`
  (`Symlink` after `may_open`'s ELOOP). **L-FS10** `ioctl` lacks `FIGETBSZ`/`FIOQSIZE`/`FIOASYNC`
  (`ioctl.rs`; `du`/`cp --sparse` probe `FIGETBSZ`). **L-FS11** `vfs-package::from_files`
  `Tree::build` recurses unbounded on hostile path depth [bin]. **L-FS12** perf: `pos_lock` taken
  even when `file_count == 1`; `OpenFile::flags()` mutex hit 2-3× per syscall (`file.rs:230,248`);
  `permission()` → `getattr()` per walk component with no negative/attribute cache
  (`vfs/src/ops.rs:63-75`, `namei.rs:162-164`); one global waker list for all file locks
  (`vfs/src/locks.rs:225-230`). [base]
- **L-NET1** `shutdown()` on a reset connection returns 0 (Linux ENOTCONN; `tcp.rs:715-719`).
  **L-NET2** ephemeral `bind(0)` has no relaxed second pass (`ports.rs:194`). **L-NET3** UDP
  autobind after routing and EPIPE before address checks (`udp.rs:319-361`). **L-NET4** netlink
  `connect`/`shutdown` accept anything (`net.rs:381,953-956`). **L-NET5** `MSG_WAITALL` on Unix
  streams crosses credential changes (`net.rs:692-705`). **L-NET6** `accept4` installs the fd
  before copying the peer address (`net.rs:296-329`). **L-NET7** data path: four copies on loopback
  with per-byte `VecDeque<u8>::extend` (`stream.rs:246`), `MSG_PEEK` clones the datagram, `pump`
  holds `StreamState` across host syscalls (`reactor.rs:245-350`), one-small-read kicks
  (`:510-515`), `SCRATCH` is a process-wide static. **L-NET8** DESIGN "ping works … unprivileged
  ICMP sockets" is false (`net.rs:428-432` EPROTONOSUPPORT); dead `PortTable::is_bound`,
  `Domain::of`, `Net::interface_table`; `output_len` reports 0 for host streams. [base]
- **L-W1** `SIGWINCH`/`SIGCHLD`/`SIGCONT`/`SIGURG` never reach v1 handlers (`signal.rs:50-59`,
  by design; list in the compatibility promise — `less`/`vim`/bash rely on them). **L-W2** epoll
  side table and compat socket timeouts survive `close`/`dup`/`fd_renumber`
  (`calls/epoll.rs:70-149`, `sockopt.rs:120-152`). **L-W3** abstract Unix addresses cut at the
  leading NUL (`abi/sock.rs:301-304`). **L-W4** [bin] every file a v1 program creates is `0777`
  (`path.rs` FILE_MODE) — `execve` on a download gives ENOEXEC not EACCES; restrict or document.
  **L-W5** a module naming a bogus signal callback without `__wasm_signal` dies with an internal
  error instead of the default action (`instance.rs:175-193`). **L-W6** `imports.rs` lists a
  4-arg `wasix_32v1::sock_accept` wasix-libc never imports. [base]
- **L-CLI1** [bin] commands hidden by `--volume x:/bin` or an `fs` volume at `/usr` fail with
  ENOENT for the package's own entry — Linux-consistent, but fail early with a message.
  **L-CLI2** [bin] non-WASI-runner commands of dependencies become plain executables
  (`machine.rs` ~214-250). **L-CLI3** [bin] dead/duplicated: `Kernel::exec` one-line wrapper,
  `Compile` default in two places, `Machine.loader` vs `abis.loader()`, `MachineProgram.argv0`.
  **L-CLI4** misleading docs/help: `--entrypoint` "module" (it is the command name); precompiled
  artifacts run without preemption points (only the 3-strike exit interrupts a busy loop) — say so
  in `--help`; `api/src/lib.rs:428-429,548-549` signal docs omit `run_module`/`run_file`.
  **L-CLI5** test gaps: no test for an `execve`'d module no ABI serves, for `EBUSY` after
  `signaller()`, for `run_file` leaving no zombie;
  `run_wasixv2_package_with_dependency_commands` needs the network.
- **L-X1** `exec/src/asyncify.rs:294-305` `settle`: a trap while `unwinding` is `Some` leaves
  `moving()` true for the instance's life, so preemption points stop delivering on it. Clear
  `unwinding` on an `Err` result. **L-X2** a vfork child's trap kills the parent with SIGSEGV
  (`exec/src/process.rs:276-293`); documented, list in the compatibility promise. **L-X3**
  `exec/src/process.rs:202` compiles the exec'd image synchronously on the guest thread (base
  only; bin-commands routes through the loader). **L-X4** the `WaitQueue`/`Pollable` contract
  that entries never take socket locks (needed because `tcp.rs:360,368,500` wake under
  `TcpSocket.inner`/`Listener.inner`) is unstated; document on `WaitEntry`. [base]

---

## 3. Work packages

Each is one coherent unit for one implementation agent. Branch column says where the change
lands (base = on top of feat/wasixv2; a branch name = fix on that branch before its merge).

| WP | Branch | Scope | Findings |
| --- | --- | --- | --- |
| **WP1 exec/de_thread correctness** | base (+ rebase onto bin) | Wait for siblings before `exec_commit` (leader-side commit when the caller is not the leader); EAGAIN on an exiting group and drain `tree.images` in `forget_task`; tolerate a taken-over task in `signal_return`/`TaskSignals::with`; `exec_reset` clears `saved`/`notify`; abort `SwitchTask`/`Fork` children when an exit is queued; `may_signal` lock copy; argv/env as bytes; `kernel_execve` takes the `Exec` specifically (bin) | H3, H4, M-P1, M-P2, M-P3, M-P5, M-P8, L-P2 |
| **WP2 loader as a binfmt registry + exec cost** | bin-commands | `Loader::probe` before commit → ENOEXEC; `LoadError::Format(Errno)`; `Image { bytes, identity }`, cache keyed on identity/raw hash, instrument only on miss, 256-byte probe, no full-file copy before the format check, no 4 GiB allocation; single compile in `wasmer run`; `run_file` = `kernel.exec` + the one image loop; seed `CachedLoader` with the CLI engine, drop the duplicate `loader`/engines; move v1 command defaults to the command file in the registry (`Arc<RwLock<Commands>>`); fix loader/DESIGN docs; `from_files` depth cap | H6, H10, M-CLI2, M-CLI3, M-W5, L-CLI3, L-CLI4, L-FS11 |
| **WP3 futex + mm** | base | mremap overlap EINVAL; `Entry` carries its key + drop guard; per-key buckets, one interrupt registration per wait; `CMP_REQUEUE` compare under lock; `SysErr::RestartNoIntr` + unconditional rerun in `complete` (then make `atomic_wait` untimed use it); `exit_thread` on `exit_group`/release and `futex_exec_release`; `checked_add` on guest pointers; real check in `copy_out`; page-wise `Segment::read`; DESIGN futex-key bullet; L-F1..L-F10 as time allows | H2, M-F1..M-F7, M-SIG1, L-F* |
| **WP4 fs/vfs** | base (merge usr first) | `RESOLVE_IN_ROOT` start/root fix + test; SIGPIPE on partial pipe writes; reusable I/O scratch (no 64 KiB zeroing); full-buffer stream reads; `F_SETLK` close-race release; epoll bulk copy-out with requeue; eventfd write length; fallocate/flock/sendfile/inotify/ioctl/copy_file_range lows; stale poll.rs TODO | H1, M-FS1..M-FS5, L-FS1..L-FS10, L-FS12 (pos_lock/flags atomics) |
| **WP5 host network bridge** | base | Datagram tx backpressure (writable interest or reactor-pumped queue, honest `poll`); replay `SockOpts` onto new/accepted backend streams; gate host `accept()` on guest queue room and derive host backlog from `listen`; `break` on persistent recv error; validate/apply send cmsgs; uid through `bind`/`listen` for `SO_REUSEPORT`; L-NET1..L-NET7 | H7, H8, M-NET1..M-NET4, M-NET6, L-NET* |
| **WP6 Unix sockets** | base | Per-uid in-flight accounting with `CAP_SYS_RESOURCE` bypass, then `unix_gc` mark/sweep over queued `Msg.rights`; symmetric `getsockopt` via `SockOpts::get`; `SO_PEERCRED` -1 unconnected; `MSG_WAITALL` credential boundary | H9, M-NET5, L-NET5 |
| **WP7 wasix1 correctness** | base | Scratch bounds (fault, no resize-on-write) + clamp v1 memory maximum below `SCRATCH_BASE` and reword DESIGN; `ProcState::fork` copies fs/net tables; signal "blocked" export semantics that keep processes killable + document the libc bug; default `/etc/resolv.conf` from host nameservers + fast-fail loopback; `tty_set` line_feeds; bogus-callback fallback to SIG_DFL; drop epoll/timeout records on close/renumber; preopen fd-3 behaviour documented or re-homed; L-W3..L-W6 | H5, H11, H12, M-W1, M-W2, M-W4, L-W* |
| **WP8 syscall hot-path perf** | base | wasix1: pid cached per instance/vfork map, reusable scratch per instance, `nbytes = 0`, one `rt_sigaction` pass, imports filtered by module, errno table; exec: generated imports filtered by `module.imports()`, unboxed/sync guest-copy fast path or per-syscall batching; measure with a `getpid`/`read` microbench before and after | M-W3, M-X2, M-X3 |
| **WP9 signals: sigaltstack, limits, pdeathsig, lows** | base | `on_sig_stack(sp)` from exec + `SS_AUTODISARM`; `RLIMIT_SIGPENDING` for RT signals; `PR_SET_PDEATHSIG` delivery; leader exit status; altstack inheritance; `CLONE_PIDFD` EINVAL (or implement); pidfd zombie/info; vfork parent killable/deferred `process_exited` | M-P4, M-P6, M-P7, M-P9, L-P1, L-P3, L-P4, L-P5 |
| **WP10 facade/CLI** | base + bin | Ctrl-C: unanswered counting from the kernel's answer; scripts: auto-mount script dir, `--use` via `install_commands`, default `PATH`/`HOME`; mounting: downcast first, boot once, skip empty layer, cache validation; one tokio `Handle` + `LocalSet` per guest thread; `Machine::shutdown()`; L-CLI1/2/5 | M-CLI1, M-CLI4, M-CLI5, M-CLI6, L-CLI* |
| **WP11 robustness** | base | Poison-tolerant locking (`parking_lot` or helper); `instrument()` failure at `warn` + reported to the embedder; `settle` clears `unwinding` on error; `WaitEntry` contract doc; DESIGN `ping`/futex-key corrections | M-X1, M-X4, L-X1, L-X4, L-NET8, L-F11 |
| **WP12 libc2 vfork** | libc2 userland-fixes | `deprecated` attribute on the function declaration, delete the stale comment, DESIGN row notes ENOSYS without asyncify; merge libc2 and rebuild the sysroot before the wasmer bump | M-LIBC1 |

Merge verdicts: **v1-sigterm** — merge as is (M-SIG1 polish folds into WP3). **userland-fixes** —
merge as is (M-FS6 verified correct; M-USR1 is a pre-existing bound to revisit; M-LIBC1 is a
libc2-side nit; keep merge order libc2 → sysroot → wasmer). **bin-commands** — merge after WP2's
H6 (ENOEXEC probe) and M-P8 (`kernel_execve` pop) at minimum; H10/M-CLI2/M-CLI3/M-W5 can follow on
top but are cheaper to do before the branch spreads.

---

## 4. Lock-ordering model (as inferred; for the WP authors)

Outer → inner. `Kernel.state` (task table) → `procs.tree` → `Process.ident`; `state` →
`thread::Group.state`; `state` → `Signals.state`; `state` → `FdTable`/`Process.{args,env,cred,
resources}`. `Signals.state` is dropped before `PosixTimer::deliver`/`force_fatal_signal`; timers
go `PosixTimer.state` → `Signals.state` → `Timers.shared.state`. Futex: `Futexes.pi` (async) →
`Futexes.queues` (global std) → `WaitQueue` → `TaskState.inner` → waker. Mm: `Mm.op` (async) →
`Mm.space` → guest memory; `op` → VFS inode locks via write-back. IPC: table `ids` → object
`state` → `perm` → (shm) `total`; object `state` → `Signals.state` (`mq_notify`). VFS:
`Superblock.rename_lock` → parent `Inode.lock` (ancestor first, else by address) → victim →
`Inode.state`/`Dentry.inner` (parent → child) → driver; `seal_lock` leaf. Readiness: object state
mutex released before `WaitQueue::wake`; entries run under the queue lock → epoll `Ep.state` →
`Ep.wq` → parent `Ep.state`. Net: file table → socket; `TcpSocket.inner` → `Listener.inner` →
`Conn.inner`/`StreamState`/`Net.state` (leaf; `Weak` upgrade outside); Unix connecting → listener
→ embryo; reactor `Shared.*` leaves, wakes after release. Violations found: M-P1 (`may_signal`),
L-F5 (contract), and the unstated L-X4 invariant that wait entries never take socket locks.

---

## 5. Top 5 gaps vs core Linux for a polished v2

1. **Job control and the controlling terminal.** Stop signals are consumed and dropped
   (`signal.rs:957-958`), no `SIGCONT`/`WSTOPPED`/`WCONTINUED`, no orphaned-pgrp `SIGHUP`, no
   `tty`/foreground pgrp/`SIGTTIN`/`SIGTTOU`, no line discipline or pty. Shells (`fg`/`bg`,
   Ctrl-Z), `less`/`vim` resize, and a correct Ctrl-C all depend on this; the CLI's pgrp hack
   (M-CLI1) is a stopgap. DESIGN §Terminals promises it.
2. **exec semantics before the point of no return.** ENOEXEC from a real binfmt registry (H6),
   siblings dead before commit (H3), EAGAIN on an exiting group, argv as bytes, and identity-keyed
   image caching so `execve` is cheap enough for shell loops (H10). Plus `wasmer run file.wasm`
   starting through `execve` of a guest-visible file so `/proc/self/exe` and re-exec work.
3. **Process accounting and CPU time.** `rusage` is zeroed in `waitid` (`process.rs:782-785`),
   `ITIMER_VIRTUAL`/`PROF` never fire, `CLOCK_*_CPUTIME_ID` is wall time since start, no
   `RLIMIT_CPU`/`SIGXCPU`, `times()` empty; `PR_SET_PDEATHSIG`, `CLONE_PIDFD`/`clone3`,
   `pidfd_getfd` missing; one flat pid space with no reuse or pid namespaces.
4. **Networking beyond the socket layer.** No inbound UDP from the host and no guest/host UDP
   port relation; accept-queue vs host backlog decoupled (M-NET1); no error queue / ancillary
   delivery (`MSG_ERRQUEUE`, `IP_RECVERR`, `IP_PKTINFO`, `SO_TIMESTAMP*`); no `unix_gc`, `MSG_OOB`,
   `SO_PEEK_OFF`; no `TIME_WAIT`, urgent data, self-connect; `TCP_INFO` synthesized; DNS not
   provided by default (H11).
5. **VFS/fs completeness and caching.** No dcache/attribute/negative cache (every walk component
   is a driver `lookup`+`getattr`, on `vfs-host` an `fstatat`+`openat(O_PATH)` each), no `mount`
   family or propagation, no `splice`/`tee`/`vmsplice`, no `F_SETOWN`/`FASYNC`/`F_SETLEASE`/
   `F_NOTIFY`, no `max_user_watches`/`reverse_path_check`, `flock` only on inode files, and the
   MMU-less mapping model's coherence points (documented) with its O(bytes) sync cost (M-F7).
