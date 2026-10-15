# WASIX Redesign: Linux-Compatible Kernel

## Goals and non-goals

We are building a new kernel for wasm guests, designed from scratch, that aims for full compatibility with Linux userspace. The current `lib/wasix` is not a reference for any part of it.

Full compatibility is a goal to steer by, not a state we will reach; wasm's limits rule it out.

- **Low-level code** is ported to `wasixv2` explicitly: libc, language runtimes, and crates such as mio and tokio.
- **Application code** should build unchanged most of the time.

**Goals**

- A program that runs on Linux, compiled for wasm, behaves the same: syscall semantics, errno values and their precedence, struct layouts, `/proc`, signals, process trees, terminals.
- Strict layering: guest ABI, then the syscall trait, then optional layers, then the kernel, then host backends. Dependencies only point downward.
- The kernel does not depend on any wasm engine and can be tested from plain Rust.
- Tracing, recording and policy plug in as layers, never as code inside syscalls.
- Many isolated machines can run in one host process.

**Non-goals**

- Copying Linux's internal names or structure. Linux is the compatibility target and a source of ideas, not a naming scheme.
- WASI preview 1. It is dropped unless it can be added later without any change to the core.
- Native ELF execution, kernel modules, and anything else wasm cannot express (see the limits section).

## Architecture overview

A syscall leaves the guest as a typed import call, is decoded by the execution layer, passes through optional layers, and lands in the kernel, which uses filesystem, network and host backends.

```text
guest (wasm32, libc2)
  │  typed import call, module `wasixv2`
  ▼
wasixv2-exec        decode arguments, suspend while pending, return-to-guest hook
  │  Syscalls trait
  ▼
layers              trace · record · policy (optional, composed as generics)
  │  Syscalls trait
  ▼
wasixv2-kernel      tasks, processes, signals, memory, IPC, terminals, sockets
  │
  ├── wasixv2-vfs   ── vfs-mem · vfs-host · vfs-overlay · vfs-package
  ├── wasixv2-vnet  ── vnet-host · vnet-switch
  └── wasixv2-host     executor, clocks, timers, entropy, byte streams
```

Only the execution and dynamic-linking crates know about the wasm engine. Everything from the layers down is plain Rust and can be tested without wasm.

## Guest ABI

Every syscall is its own typed wasm import. A module built for a newer kernel therefore fails at link time on an older one, instead of sometimes running until it reaches a missing syscall.

- **Why typed imports:**
  - Link-time failure on version skew.
  - The wasm validator checks argument types.
  - No dispatch switch on a syscall number.
- **Names:**
  - Every import lives in the module `wasixv2`, syscalls and wasm-specific extensions alike.
  - Function names are the WIT identifiers: kebab-case Linux names such as `openat`, `clone3` and `clock-gettime`.
  - Linux's `64` suffixes are kept where they exist. Because the ABI is our own, a call can also use 64-bit time under its plain name, as `clock-gettime` does.
- **Arch:** wasm32 only. It is ILP32: `int`, `long` and pointers are 32 bits, as on every other wasm32 target.
- **Target triple:** `wasm32-wasixv2`, which LLVM spells in full as `wasm32-unknown-wasixv2`, the same way `wasm32-wasip2` expands.
- **Source-level identity:** the target is unix but not Linux. In Rust it is `target_family = "unix"` with `target_os = "wasixv2"`; clang defines `__unix__` but not `__linux__`. Crates and C code that gate on Linux (libc, nix, mio, tokio) need an explicit `wasixv2` arm.
- **Table and layouts:** modelled on Linux's generic syscall table (`scripts/syscall.tbl`) and on riscv32, without copying either:
  - 64-bit `time_t` everywhere, with no separate 32-bit-time variants.
  - 64-bit `off_t`.
  - `statx` for file metadata.

  errno values, signal numbers and flag bits come from `asm-generic`.
- **Page size:** `AT_PAGESZ` and `getpagesize` report 64 KiB, the wasm page size.
- **Defined by us:** these are things no Linux ABI defines for wasm:
  - the signal context (`ucontext_t`, `mcontext_t`)
  - what `CLONE_SETTLS` sets
  - auxv hardware-capability bits for wasm features
  - `uname -m` reporting `wasm32`
- **Coverage:** the kernel exports the whole table. Syscalls it cannot support are still exported and return ENOSYS, as Linux does for compiled-out syscalls.
- **No optional imports:** libc follows a minimum-kernel policy, like glibc's. It imports a syscall only once the oldest supported kernel provides it, so there is no ENOSYS probing.
- **No version suffixes:** Linux never changes a syscall's signature; it adds a new syscall instead (`clone3`, `openat2`). Name plus signature is stable.
- **Multiplexed syscalls** (`ioctl`, `fcntl`, `prctl`, `futex`) keep their untyped argument, and the kernel decodes it by op code.
- **Wasm-specific extensions** sit in `wasixv2` next to the syscalls. Examples are vfork support, loading a side module, and starting a thread on shared memory.

## The syscall trait

A syscall runs as: imported function → decode arguments → method on a trait with one method per syscall → zero or more layers → the kernel. Five rules keep that path Linux-exact.

1. **User memory is passed as references, not decoded.** Scalars are decoded eagerly. Buffers, strings and structs arrive as `UserPtr<T>`, `UserSlice<T>` or `UserCStr`, and the kernel copies them in and out on demand through the calling task's memory.
   - This keeps Linux's error precedence: `read(-1, NULL, 10)` returns EBADF, not EFAULT.
   - It allows partial results when a copy faults partway through.
   - `ioctl`, `futex`, `recvmsg`, `execve` and `select` cannot be decoded ahead of time anyway.
2. **Flags stay as raw bits.** Flag types are transparent newtypes such as `OpenFlags(u32)`, and validation is the kernel's job. Linux ignores unknown `open` flags but rejects them in `openat2`, and the decoder must not be stricter than Linux.
3. **Methods are async, and restarting is part of the result.** The error type is `SysErr::Errno(..)` or `SysErr::Restart(..)`. Only the outermost glue turns a restart into rerunning the call or returning EINTR, following `SA_RESTART` and `restart_syscall` semantics.
4. **Control flow goes through a return-to-guest hook, not the return value.**
   - Affected calls: `exit`, `execve`, `clone`, `rt_sigreturn` and signal delivery.
   - The kernel records the pending work on the task.
   - The engine glue checks the task on every return path and does the work: build a signal frame, swap the image, unwind.
5. **Everything is generated from one table.**
   - From one syscall table, written in the component model's WIT format, codegen produces the wasm import shims, the decoders, the `Syscalls` trait and a `Syscall` request enum.
   - It also produces a `Layered<L, S>` adapter that forwards every method by default.
   - The table and the generator live in their own repository, [wasixv2-abi](https://github.com/wasix-org/wasixv2-abi), which wasmer mounts as a submodule at `lib/wasixv2/codegen` and libc2 at `wasixv2/abi`. Each consumer commits the code it generates.

```rust
// generated
pub trait Syscalls {
    async fn read(&self, t: &Task, fd: Fd, buf: UserSliceMut<u8>) -> SysResult<usize>;
    async fn openat(&self, t: &Task, dir: Fd, path: UserCStr, flags: OpenFlags, mode: Mode) -> SysResult<Fd>;
    async fn futex(&self, t: &Task, addr: UserPtr<u32>, op: FutexOp, val: u32, timeout: UserPtr<Timespec>, addr2: UserPtr<u32>, val3: u32) -> SysResult<isize>;
    // one method per syscall in the table
}
```

**Layers come in two kinds.**

- **Typed layers** override only the methods they care about, such as a mount policy on `openat`.
- **Generic layers** (trace, record, policy, metrics) implement one `call(task, Syscall, next)` over the request enum.

Layers compose as generics, e.g. `Record<Trace<Kernel>>`, so dispatch is static and inlined. A layer that can be switched at runtime is a disabled no-op, not a `dyn` stack, which would box a future on every syscall.

**Recording still sees memory writes.** A recording layer hands the inner layers a `UserMemory` that records writes, so it captures every byte the kernel copies out to the guest.

## Kernel model

The kernel is plain async Rust with no wasm engine, no async runtime and no recording code in it. It owns every piece of state a Linux process can observe.

- **Tasks and sharing.** Every thread is a task. A task holds shared references to resource groups, and `clone` flags decide which groups a child shares and which it copies:
  - address space
  - descriptor table
  - filesystem context (cwd, root, umask)
  - signal handlers
  - thread group

  A process is a thread group. Fork, vfork and thread creation are all the same operation with different flags.
- **Process tree.**
  - Parent and child links, sessions, process groups and controlling terminals.
  - Zombies and reaping, `wait` variants, pidfds.
  - Pid allocation per pid namespace, with reuse.
- **Credentials.** Real, effective and saved uid and gid, supplementary groups and capabilities. They are checked by the kernel, never by the host.
- **Signals.**
  - Pending sets per task and per thread group, masks, dispositions, `sigaltstack`, and queued realtime signals.
  - Delivered on the return-to-guest path.
- **Waiting and time.**
  - Wait queues; futexes keyed by memory object and offset, so shared mappings work across processes.
  - Clocks; interval, POSIX and fd-based timers, all driven by host timers, never by polling.
- **Namespaces.** Mount, pid, network, uts, ipc and user namespaces.
- **IPC.** Pipes, Unix sockets, SysV and POSIX message queues, shared memory and semaphores, and event, signal, timer, epoll and inotify descriptors.
- **Memory.** A manager for the guest's linear memory that implements `brk`, anonymous `mmap`, private file mappings and an unenforced `mprotect`.
- **Terminals.**
  - The line discipline, pseudo-terminals and job control (`SIGTTIN`, `SIGTTOU`, foreground groups) live in the kernel.
  - The host only provides raw byte streams.
- **Pseudo filesystems.** `/proc`, `/sys` and `/dev` are in the kernel because they expose kernel state.

Open objects follow a three-level split:

1. A **descriptor** is a table slot plus close-on-exec.
2. An **open file** holds the offset and status flags, and is shared by `dup` and `fork`.
3. The **object** behind it is an inode, socket, pipe or event source, behind one trait for read, write, poll and ioctl.

### Readiness and wait queues

The crate `wasixv2-wait` holds what the VFS, the network and the kernel all need for readiness. It is the counterpart of Linux's `wait_queue_head_t`, `poll_table` and `__poll_t`.

- **`PollMask`**: the `EPOLL*` bits, which equal the `POLL*` bits on the generic architecture.
- **`WaitQueue`**: owned by an object, which calls `wake(events)` whenever its state changes, from any thread. An empty key means "anything".
  - `add(Arc<dyn WaitEntry>)` returns a guard that unregisters on drop.
  - Exclusive entries queue last. A wake stops at the first exclusive entry that takes it.
- **`Pollable::poll(Option<&mut PollTable>) -> PollMask`**: Linux's `f_op->poll`. It registers on the object's queues through the table and returns the current mask without blocking.
- **Waiting** goes through Rust wakers, so it works on any executor:
  - `Waiter` is one task's entry and can sit on many queues.
  - `PollWaiter` is `poll_wqueues`, used by poll and select.
  - `wait_event`, `wait_event_timeout` and `wait_event_exclusive` are cancellation-safe. An exclusive wakeup that a cancelled waiter took is passed on.
  - Each takes an `Interrupt` hook, the kernel's `signal_pending`.

### The object trait

`FileObject` (kernel `file.rs`) extends `Pollable` with:

- `read` and `write`, which take an `Io` holding the open file's flags as the call began plus the interrupt hook. They block asynchronously unless `O_NONBLOCK` is set, in which case they return `EAGAIN`.
- `ioctl`, which defaults to `ENOTTY`. The kernel handles `FIOCLEX`, `FIONCLEX`, `FIONBIO`, and `FIONREAD` on regular files, before calling the object.
- `can_poll`, which is `file_can_poll`.

Dropping the object is Linux's `release`. Inode-backed open files poll as `DEFAULT_POLLMASK` and are `EPERM` for epoll.

### poll, select and epoll

These follow `fs/select.c` and `fs/eventpoll.c`.

- **poll and select:**
  - Register on the first pass only.
  - `POLLNVAL` for closed descriptors, while select returns `EBADF`.
  - `POLLERR` and `POLLHUP` are always reported.
  - The time left is written back to the timespec.
  - The temporary signal mask is restored on return.
- **epoll:**
  - An item is keyed by (open file, descriptor) and goes away when the open file's last reference does, not when one duplicate closes.
  - Items wake the epoll from their queue callback. `epoll_wait` re-polls ready items, requeues level-triggered ones and disarms one-shot ones.
  - Epolls nest up to `EP_MAX_NESTS`, and a nesting that would create a loop is `ELOOP`.
  - `epoll_wait` is never restarted.
- **Not yet:**
  - Signal delivery, so waits are never interrupted.
  - `reverse_path_check`'s wakeup-path limits.
  - `max_user_watches`.

## Filesystems

There is one VFS. It owns all path and permission semantics, and filesystem drivers only store data.

**The VFS owns:**

- A mount tree per mount namespace: bind mounts, propagation, read-only mounts, overmounting.
- Path walking: symlinks, `..`, and the `openat2` resolve flags.
- A name cache.
- Permission checks from mode bits and ownership.
- File locks (`flock`, POSIX and open-file locks).
- Change notification.

**The driver trait is inode-based:**

- `lookup`
- `getattr` with full metadata (mode, owner, inode number, link count, device, nanosecond times)
- `create`, `link`, `unlink`
- `rename` with flags
- `readdir` with resumable cookies
- extended attributes
- positional read and write
- hooks for mapping

It is async throughout.

**Drivers:**

| Driver | Purpose |
| --- | --- |
| Memory | tmpfs-style, the default writable root |
| Host | Passthrough to a host directory |
| Overlay | Writable upper layer over read-only lower layers |
| Package | Read-only view of a packaged filesystem image |
| Kernel pseudo | `/proc`, `/sys`, `/dev`, `/dev/pts` (in the kernel crate) |

Mounts are the only way to compose filesystems. Overlay is a filesystem, not a second composition mechanism.

### The VFS and driver split

`wasixv2-vfs` decides every path, permission and ownership question, following Linux's `fs/namei.c`, `fs/open.c`, `fs/attr.c`, `fs/utimes.c` and `fs/xattr.c`, including their error precedence. The kernel only decodes syscall arguments, keeps per-process state (root, working directory, umask, credentials) and calls `Vfs`. Drivers never see paths, `.`, `..`, symlink resolution, credentials or permission checks: they get single names in a directory they already returned, and objects of their own filesystem.

```rust
#[async_trait(?Send)]
pub trait Filesystem: Send + Sync {
    fn info(&self) -> FsInfo;                       // statfs magic, name_max, read_only, dev, has_btime
    fn root(&self) -> Arc<dyn Node>;
    async fn statfs(&self) -> FsResult<StatFs>;
    async fn sync(&self) -> FsResult<()>;           // syncfs
}

#[async_trait(?Send)]
pub trait Node: Send + Sync + Any {                 // Linux's inode + inode_operations
    fn ino(&self) -> u64;
    fn file_type(&self) -> FileType;                // regular, dir, symlink, fifo, socket, char, block
    fn as_any(&self) -> &dyn Any;
    async fn getattr(&self) -> FsResult<Attr>;      // mode, uid, gid, nlink, size, blocks, a/m/c/btime (ns), rdev, dev
    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr>;   // values the VFS decided; size truncates
    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>>;
    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>>;  // any type, mode/uid/gid decided
    async fn create_unnamed(&self, new: &NewNode<'_>) -> FsResult<Arc<dyn Node>>;       // O_TMPFILE
    async fn link(&self, name: &[u8], target: &Arc<dyn Node>) -> FsResult<()>;
    async fn unlink(&self, name: &[u8]) -> FsResult<()>;
    async fn rmdir(&self, name: &[u8]) -> FsResult<()>;
    async fn rename(&self, name: &[u8], new_dir: &Arc<dyn Node>, new_name: &[u8], mode: RenameMode) -> FsResult<()>;
    async fn readlink(&self) -> FsResult<Vec<u8>>;
    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>>;
    async fn getxattr / setxattr(name, value, XattrSet) / listxattr / removexattr;
}

#[async_trait(?Send)]
pub trait Handle: Send + Sync {                     // file_operations for an opened object
    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize>;
    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize>;
    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)>;   // atomic O_APPEND
    async fn sync(&self, data_only: bool) -> FsResult<()>;
    async fn allocate(&self, mode: AllocMode, offset: u64, len: u64) -> FsResult<()>;
    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64>;
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()>;  // no . or ..
}
```

- **Timestamps.** Drivers keep the times their own operations imply (a write stamps `mtime` and `ctime`, a new entry stamps the parent), as Linux filesystems do in their inode operations; explicit changes arrive through `setattr`.
- **Directory cookies** resume a read while entries come and go, and stay below 2^31 so a 32-bit `telldir` holds them. The VFS adds `.` and `..` at positions 0 and 1, and a position past the end, as tmpfs's `simple_offset` readdir.
- **Open files.** `vfs::File` is the filesystem part of an open file description (path, handle, lock owner id); the kernel's `OpenFile` holds it as `Backing::Inode` with the position and status flags. Dropping the last reference is the last close.

### Mounts, names and path walking

- **Mount tree.** A `Mount` is a superblock plus the dentry its tree starts at (a filesystem root, or any directory for a bind mount) and per-mount flags (read-only, nosuid, nodev, noexec, noatime, nosymfollow). Mounts stack on a `(mount, dentry)` mountpoint and the topmost wins. The embedder builds the tree with `Vfs::mount` and `Vfs::bind_mount`; the `mount` family of syscalls is not implemented yet.
- **Dentries.** The VFS keeps the tree of names it has seen, for `..`, `getcwd`, mountpoints and the rename ancestry check. Only dentries something holds stay alive. A lookup always asks the driver and reuses the cached dentry only when it names the same object, so names that change behind the VFS (on a host directory) are never answered from a stale cache. There is no negative or attribute cache yet.
- **Walk.** `link_path_walk` semantics: search permission on every directory, symlinks followed in the middle and at the end unless `NOFOLLOW`, at most 40 per walk (`ELOOP`), trailing slashes demand a directory and force following, `..` stops at the process root and climbs from a mount root to its mountpoint, and the `openat2` scopes `RESOLVE_BENEATH`, `IN_ROOT`, `NO_XDEV` and `NO_SYMLINKS` (`RESOLVE_CACHED` resolves normally).

### Permissions

`generic_permission` over the owner, group (with supplementary groups) and other bits, plus the DAC-override capabilities; sticky directories; set-gid inheritance and the umask at creation; `chmod`, `chown` and `utimensat` ownership rules. Processes run as uid and gid 1 without capabilities. Read-only filesystems and mounts give `EROFS` where Linux does.

### Locks

`LockManager` holds `flock` locks (owned by an open file), POSIX record locks (owned by a process, dropped when it closes any descriptor of the file) and OFD record locks (owned by an open file). Record locks of both kinds conflict with each other and split and merge like Linux's; `flock` locks are separate. Blocking requests wait for any lock change, and `F_SETLKW` detects deadlock cycles between processes (`EDEADLK`). Waiting is not yet interruptible by signals.

### Change notification

Every change the VFS makes is reported to one `FsWatcher` as an `FsEvent` (create, delete, move, modify, attrib, access, open, close), keyed by object identity (superblock id, inode number). inotify instances will hang off this hook in the kernel; they are not implemented yet. Changes made on the host side of a host directory are not seen.

### Host directories

`wasixv2-vfs-host` (Linux hosts only for now) holds every object as an `O_PATH` descriptor and resolves names with `openat(dir, name, O_PATH | O_NOFOLLOW)`, one component at a time and never `..`, so neither symlinks nor `..` can leave the mapped directory: the VFS interprets symlink targets inside the guest tree. Operations that need more than an `O_PATH` descriptor go through `/proc/self/fd/N`, which names the held inode exactly, as libfuse's `passthrough_ll` does. Blocking host calls run on the `Host` executor's blocking pool. Files the host user owns appear as owned by the guest user, and the reverse.

## Networking

The kernel implements networking at the socket layer and has no TCP/IP stack. A guest can only observe sockets (ports, `bind` conflicts, accept queues, addresses, options, errno), and in-memory streams plus host sockets cover all of that.

- **Port spaces** belong to network namespaces and live in kernel tables. Ephemeral ranges, `EADDRINUSE`, `SO_REUSEADDR`, `SO_REUSEPORT` and the low-port capability check are decided against guest state. Ports the host has bound never change what a guest `bind` returns.
- **Inside a namespace:**
  - A `connect` to a local listener queues on that listener.
  - `accept` pairs the two sockets with an in-memory stream.
  - UDP datagrams move directly between socket queues.
  - Nothing is turned into packets.
- **Interfaces and routes** are kernel tables served over netlink, so `ip addr`, `getifaddrs` and routing lookups work. No frames sit behind them.
- **Namespaces:** `unshare` of the network namespace gives a fresh port space with only loopback.
- **Unix sockets** stay a separate kernel facility. Their abstract namespace is per network namespace.

**The vnet interface** (`wasixv2-vnet`'s `api` module):

- **The kernel's socket layer** decodes and encodes guest structs and owns descriptors, blocking, `SO_RCVTIMEO` and `SO_SNDTIMEO`, signals, and the `SOL_SOCKET` options that only describe the descriptor. A missing network makes IP `socket` calls fail with `EAFNOSUPPORT`.
- **`Network`** is one namespace. `socket(domain, kind, protocol)`, which is `inet_create`, plus `ports()` and `interfaces()`.
- **`Socket`** is one IP socket, and the kernel wraps it as an open-file object.
  - Operations: `bind`, `listen`, `accept`, `connect`, `disconnect`, `shutdown`, `send`/`recv` (with `Control` messages), `local_addr`, `peer_addr`, `take_error`, `input_len` and raw `set_option`/`get_option`.
  - Every operation is synchronous and non-blocking, returning `EAGAIN` or `EINPROGRESS`.
  - `poll` must match `tcp_poll` and `udp_poll`. The kernel blocks by waiting for poll events and retrying, as `sk_wait_event` does.
- **`Backend`** carries traffic leaving the namespace:
  - `connect` returns a `BackendStream`.
  - `datagram` returns a `BackendDatagram`.
  - `publish` hands accepted host connections to an `InboundSink` and returns a `Publication`.

  Backends wake the returned objects' queues from their own threads.

**Where a connection goes**

| Destination | How it is carried | Port relationship |
| --- | --- | --- |
| Same namespace, including loopback | In-memory stream between the two sockets | One guest port space |
| Another machine on a virtual switch | Connection request and stream data routed to the machine that owns the destination address | Each machine has its own address; ports are per address |
| Anything else (host backend, the default) | The kernel opens a host socket on the guest's behalf | Guest port is virtual; the host picks its own |
| No backend | Fails as an unreachable network | Loopback only |

**Publishing** is the only place host and guest ports meet.

- **Explicit publishing:** the embedder maps a host port to a guest address and port. Each connection the host accepts is handed to the guest listener.
- **Auto-publishing (opt-in):** the kernel watches `listen` and mirrors the port on the host.
- **Failure is the host's problem.** A host port that is already taken is a logged publishing failure, never an error returned to the guest.

**Policies:**

- **Gateway alias.** Whether one guest address reaches the host's loopback.
- **Peer address.** The host backend knows each connection's real peer, so `getpeername` reports it.
- **DNS.** `resolv.conf` points at resolvers the host backend can reach.

**Not supported:**

- raw and packet sockets
- netfilter
- MTU and fragmentation behaviour
- exact `TCP_INFO` values, which are synthesized

`ping` works the way unprivileged ICMP sockets do on Linux, through a host ICMP socket. A packet-level stack can come later as an optional backend if something needs these.

### Port tables

`wasixv2-vnet`'s `Net` is one namespace. It keeps one port table per protocol, shared by IPv4 and IPv6 as Linux's bind hash is, so a dual-stack `[::]` socket collides with IPv4 sockets on its port unless it is `IPV6_V6ONLY`. IPv4-mapped addresses are stored as IPv4.

- **Explicit bind** (`inet_bind_conflict`): two sockets conflict when their addresses overlap (equal, or either a wildcard that covers the other), unless both set `SO_REUSEADDR` and the existing one is not listening, or both set `SO_REUSEPORT` with the same owner. `listen` re-checks, so of two `SO_REUSEADDR` sockets on one port only one can listen. UDP shares with `SO_REUSEADDR` on both, whatever the state.
- **Errors, in Linux's order:** `EADDRNOTAVAIL` for an address on no up interface (unless `IP_FREEBIND`), `EACCES` below `ip_unprivileged_port_start` (1024) without the capability the kernel passes in, `EINVAL` when already bound, `EADDRINUSE` on a conflict.
- **Ephemeral ports** come from `ip_local_port_range` (32768–60999) from a seeded random offset. `bind` to port 0 tries odd ports first (SO_REUSEADDR sockets the lower half of the range, others the upper half) and fails with `EADDRINUSE`; `connect` tries even ports first, may share a port with other connected sockets that have a different peer, and fails with `EADDRNOTAVAIL`.
- Accepted sockets share the listener's port. Closing a socket frees its port at once: there is no `TIME_WAIT`.

### Streams and datagrams

- **TCP inside a namespace.** `connect` to a local listener creates the accepted socket at once and pairs the two with an in-memory connection. When the accept queue holds more than `backlog` connections, the client waits in `SYN_SENT` (`EINPROGRESS`) until `accept` makes room. Closing the listener resets queued connections and refuses waiting clients. A connection to a port nobody listens on fails with `ECONNREFUSED` right away.
- **Buffering.** Each direction queues up to the receiver's `SO_RCVBUF` plus the sender's `SO_SNDBUF`, counted in payload bytes. `SO_SNDBUF` and `SO_RCVBUF` are doubled and clamped as Linux does. Writes are partial when the queue fills, then `EAGAIN`.
- **Close and shutdown.** These follow `tcp_close` and `tcp_reset`. Closing with unread data or `SO_LINGER` zero resets the peer: its next call fails with `ECONNRESET`, or `EPIPE` once it has seen our FIN, and later calls see EOF or `EPIPE`. A write to a peer that closed normally succeeds once and then fails with `EPIPE`.
- **Readiness** is `tcp_poll`'s: a fresh socket is `OUT|WRNORM|HUP`, `RDHUP` follows the peer's FIN, `HUP` needs both directions shut, and `ERR` shows while `SO_ERROR` is set.
- **UDP.** Datagrams are copied between receive queues. A queue drops new datagrams once it holds more than `SO_RCVBUF`, charging each payload plus a fixed 768-byte overhead. Connected sockets only receive from their peer. A connected sender learns `ECONNREFUSED` when nobody is bound to its peer's port. `MSG_PEEK` and `MSG_TRUNC` work. Broadcast and multicast are accepted but not delivered.

### Backends and the host bridge

The kernel-facing traits are in `vnet/src/api.rs`. Traffic for a destination that is not local goes through a route on the uplink interface (by default `eth0`, 10.0.2.15/24 via gateway 10.0.2.2), so the guest's source address is the uplink address and a virtual ephemeral port. Without a route the error is `ENETUNREACH`.

- The **gateway alias** (by default the gateway) maps to the host's loopback in both directions.
- **Publishing** (`Net::publish`, or auto-publishing of non-loopback listeners) hands `Backend::publish` a sink that delivers each accepted host connection to whichever guest listener holds the address at that moment. `getpeername` reports the real host peer.
- **DNS**: `NetConfig::nameservers` makes up `resolv.conf`. `vnet-host::host_nameservers` derives them from the host's file and routes loopback resolvers through the alias.

`wasixv2-vnet-host` runs one mio reactor thread per `HostBackend`, never tokio, and is Unix-only for now.

- Guest-side stream and datagram objects are buffers: 256 KiB per direction for streams, 256 KiB of queued datagrams. Guests read and write them without blocking.
- The reactor moves bytes between the buffers and the host sockets. Guest activity marks a stream dirty and wakes the reactor through a `mio::Waker`. The reactor wakes guest wait queues from its own thread.
- Host errors keep their Linux errno numbers. Dropping the guest's handle flushes and closes the host socket, or resets it when unread data or `SO_LINGER` zero call for that.

## Execution

The execution crate is the only layer below the facade that knows about the wasm engine. It turns kernel tasks into running wasm, and turns guest control flow into kernel operations.

- **Loading.**
  - Wasm modules and `#!` scripts.
  - A registry of further formats, like binfmt\_misc.
  - The kernel resolves the path and permissions; the loader only interprets bytes.
- **Tasks and instances.**
  - A process image is one instance.
  - A thread created with a shared address space is another instance on the same shared memory.
  - Each task runs on a host thread or a coroutine, chosen by the host crate.
- **Suspension.** Syscall methods are async. A pending call suspends the guest with stack switching, which every wasmer backend we target supports. It works the same in every build configuration.
- **Build configurations.** All three work from day one:
  - **Exception handling with exnref** (the default). `setjmp`/`longjmp` use wasm exceptions.
  - **Exception handling plus PIC.** For programs that need dynamic linking.
  - **Asyncify.** For programs that need `fork`, which today means bash, and for guests that will be checkpointed. Asyncify also implements `setjmp`/`longjmp` in this configuration.
- **Return to guest.** After every syscall and at every preemption point, the hook runs pending work:
  - deliver signals: build the frame, run the handler, and have `rt_sigreturn` restore state
  - restart interrupted calls
  - swap the image after `execve`
  - tear down on exit
- **Signal frames.** The interrupted wasm call stack stays intact under the handler, so the frame is small.
  - **`mcontext_t`** holds only guest state a handler can clobber, such as the shadow stack pointer and the TLS base.
  - **`sigaltstack`** moves only the shadow stack.
  - **`rt_sigreturn`** lives in libc as a `setjmp`/`longjmp` pair around the handler call.
  - **`siglongjmp` out of a handler works across host frames.** The embedder's `Function::call` returns an error carrying the wasm exception, and the import returns it, which re-raises it through the remaining wasm frames. The host-side signal delivery code must pass these errors through untouched.
- **Coroutines.** wasixv1's context API (`context-create`, `context-switch`, `context-destroy`) carries over as `wasixv2` extension functions, and greenlet keeps using it directly. libc implements `getcontext`, `setcontext`, `makecontext` and `swapcontext` on top of it. On each switch, libc also swaps the shadow stack pointer and signal mask, passes arguments through a trampoline, and follows `uc_link`.
- **Fork.** Fork needs the asyncify configuration and goes through clone without CLONE\_VM. Asyncify unwinds the stack into linear memory, memory is copied, and both are rewound, so clone returns the child's pid in the parent and 0 in the child.
- **Vfork.** This is salvaged from wasixv1's setjmp-based vfork and works in all three configurations:
  1. libc's `vfork` is a macro that calls `setjmp` in the caller's frame, then calls clone with CLONE\_VM | CLONE\_VFORK, which returns 0.
  2. The kernel creates the child with a shared address space and blocks the parent task, as `CLONE_VFORK` does on Linux.
  3. The exec layer switches the instance's current task to the child, so later syscalls run as the child.
  4. When the child calls `execve`, the new image starts in its own instance. When it calls `exit`, it becomes a zombie.
  5. Either way, the instance switches back to the parent task and the call returns. libc then `longjmp`s to the vfork point, which returns the child's pid.

  wasixv1 is brittle because it swaps its whole environment object in place and threads vfork state through exit, exec and process startup. Here a task switch is one pointer in the exec layer, and the kernel tracks the vfork relationship. As on Linux, returning from the function that called `vfork` is undefined. Because `vfork` is a macro, its address cannot be taken.
- **Preemption.** Engine interruption checks such as epochs let the kernel deliver signals to a busy guest and enforce `SIGKILL` and `SIGSTOP`.
- **Dynamic linking.** Side modules are loaded and linked by the host; see the next section.

## Dynamic linking

The host instantiates and links side modules; libc2 decides policy and runs user code. On Linux all of ld.so is userspace, but wasm has no instruction to compile or instantiate a module, so whatever creates code must be the host. The work lives in `wasixv2-dylink`, next to the exec layer: it is engine glue, and the kernel stays engine-free.

Three things make the host's part more than one "instantiate" call:

- **Threads.** Each thread is its own instance on shared memory. A library one thread loads must be instantiated into every other thread's instance, and into threads created later. Only the host sees all of a process's instances.
- **Startup.** A dynamic main module imports from its libraries, so they must be loaded before any guest code runs, which is what `PT_INTERP` does on Linux.
- **Per-instance state.** Each instance has its own `__stack_pointer`, `__tls_base`, and memory and table bases.

**The split**

| Host (`wasixv2-dylink`) | libc2 |
| --- | --- |
| Instantiate a side module in every instance of the process, and in new threads | The search list: `LD_LIBRARY_PATH`, the defaults |
| Read `dylink.0`, allocate the module's memory and table space, resolve its imports against what is loaded | `RTLD_*` flags, `dlerror()` |
| Walk the search list for the library and its dependencies | Run constructors |
| Load the main module's dependencies at startup | |

**No calls into user code during a link.** The host runs the link inside a syscall, holding the process's loader state and with other threads stopped. A call into guest code from there could re-enter the loader through a recursive `dlopen`, deadlock on locks the guest holds, unwind host frames with `longjmp` or an exception, or take a signal. So the host calls only functions wasm-ld synthesizes, which touch memory and globals and call nothing outside the linker's own output:

- `__wasm_init_memory`, the side module's wasm `start` function. An atomic flag makes it copy `.data` and set up the first instance's TLS only once per process.
- `__wasm_apply_data_relocs`, once per process.
- `__wasm_init_tls(block)`, in every other instance. It sets `__tls_base`, copies the TLS image with `memory.init`, and calls `__wasm_apply_tls_relocs`.

Side modules must export `__wasm_init_tls`, `__tls_size` and `__tls_align`; wasixcc adds the flags when it links a wasixv2 shared library.

**Memory comes from the host, never from `malloc`.** A side module's data region and every TLS block for it come from a host-owned arena, carved from pages the kernel's address-space manager hands out, as for `mmap`. `malloc` never sees those pages. This removes a deadlock that a guest allocator invites: a thread stopped while holding the `malloc` lock cannot also allocate its own TLS. The arena frees a thread's blocks when the thread exits. The main module's TLS for new threads stays with `pthread_create`.

**Constructors are the only user code, and libc2 runs them.** The host queues each newly loaded module, in initialization order. After the link syscall returns, libc2's `dlopen` takes each queued module's `__wasm_call_ctors` from the queue, as an entry in the function table, and calls it; `__libc_start_main` does the same for preloaded libraries before `main`. No host lock is held while constructors run, so they may call `dlopen` themselves. Code is never unloaded, so `dlclose` only drops a reference, and destructors run at exit.

**Policy is data.**

- `RTLD_GLOBAL`, `RTLD_LOCAL`, `RTLD_NOLOAD`, `RTLD_NODELETE`, `RTLD_NOW` and `RTLD_LAZY` are flags with musl's values. The host keeps the global scope: the main module and `RTLD_GLOBAL` modules, in load order. Linking is always immediate.
- `RTLD_DEFAULT` and `RTLD_NEXT` need the caller's module. libc2 passes an address in the caller's data, such as its `__dso_handle`, and the host maps it to a module by memory range.
- Symbol precedence and weak symbols follow ELF rules in the host.
- The host's error messages go straight to `dlerror()`.

**Search paths.** libc2 passes the ordered search list with each request: `LD_LIBRARY_PATH` as read at startup, then the defaults. The host walks it for the library and for each dependency, the latter also searched in the needing module's runtime path. A name containing `/` is opened as is. Every candidate is opened through the same kernel path as `openat`, with the process's credentials, working directory and mount namespace, so permissions and errors match `openat` exactly, and the bytes come from the open file. At `execve`, before any guest code runs, the host builds the list itself from the new environment's `LD_LIBRARY_PATH` and `LD_PRELOAD` and the same defaults, as ld.so does at startup.

**Stopping the other threads.** Each thread's function table and side-module instances must be updated before a `dlopen` returns. The loader marks every other task of the process with pending link work and wakes it. This uses the signal machinery, without a signal number:

- Injected interrupt points and interruptible waits notice the work, as they would a signal.
- The task does the work on its return-to-guest path, then continues.
- An interrupted syscall always restarts, since nothing happened that the guest can see.

A real signal would let the guest block, ignore or catch the request, and a thread that blocks all signals would stall every `dlopen`; this is Linux's `task_work`. The linker-generated functions above get no interrupt points, and run with pending work suppressed, so no thread is interrupted in the middle of setting up its own TLS.

### Host and executor

The kernel never names an async runtime. Everything it needs from the host is one `Arc<dyn Host>`, passed to `Kernel::new`; the facade builds it with `Machine::with_host(host)`, or `Machine::new()` for the tokio default.

```rust
pub trait Host: Send + Sync {
    fn realtime(&self) -> Duration;                 // CLOCK_REALTIME, since the epoch
    fn monotonic(&self) -> Duration;                // CLOCK_MONOTONIC, never goes backwards
    fn sleep(&self, d: Duration) -> LocalFuture<'_, ()>;
    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()>; // monotonic deadline; never early
    fn random(&self, buf: &mut [u8]);
    fn executor(&self) -> &dyn Executor;
}
pub trait Executor: Send + Sync {
    fn spawn_local(&self, f: LocalFuture<'static, ()>);   // detached, on the current thread's executor
    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()>;
}
// helper: (dyn Executor)::run_blocking(|| ..) -> T
```

- **Futures are not `Send`**, because wasmer's async host calls are not, so background work is spawned locally.
- **Absolute timeouts** (`clock_nanosleep` with `TIMER_ABSTIME`, epoll and ppoll deadlines) use `sleep_until` on the monotonic clock. Its default re-sleeps until the clock passes the deadline.
- **Boxing** happens once per sleep, spawn or blocking call, never per syscall.

| Implementation | Feature | Clocks | Timers | Entropy | Executor |
| --- | --- | --- | --- | --- | --- |
| `TokioHost` | `tokio` (facade default) | `SystemTime`, tokio `Instant` | tokio timers | OS (`getrandom`) | `spawn_local` needs a `LocalSet`; blocking work on tokio's blocking pool |
| `WebHost` | `web`, `wasm32-unknown-unknown` | `Date.now()`, `performance.now()` | `setTimeout`, re-armed until the deadline | `crypto.getRandomValues` | `wasm_bindgen_futures::spawn_local`; blocking work runs inline and stalls the event loop |

`Stdio::from_fns` wires stdout and stderr to callbacks with stdin at end of stream, which is enough for a browser console. A richer browser stdin is the embedder's own `ByteSource`.

**Open items**

- **Guest threads.** `Executor` gets a method to start a task on a host thread, or on a web worker, when `clone` lands.
- **Exec in the browser.** `wasixv2-exec`, `wasixv2-dylink` and the facade build for `wasm32-unknown-unknown` with the `js` feature, since wasmer's js backend implements async host functions and `call_async` with JSPI (`WebAssembly.Suspending` and `promising`). Running a guest has not been tried yet. It needs a JSPI-capable engine, and guests built with exception handling need exnref support.
- **Shared memory.** libc-built guests import a shared memory, which needs `SharedArrayBuffer` and therefore a cross-origin isolated page (COOP and COEP headers).

## Recording, replay and checkpointing

These are two separate features. Recording is a syscall layer; checkpointing serializes kernel state directly.

| Feature | Where | How | Used for |
| --- | --- | --- | --- |
| Record and replay | Generic syscall layer | Logs each call, its result and every byte copied to the guest. Replay returns the logged results without running the kernel, as rr does. | Deterministic debugging, auditing, reproducing failures |
| Checkpoint and restore | Kernel crate plus a checkpoint crate | Serializes the kernel object graph and guest memory, then rebuilds them on restore, as CRIU does | Snapshots, fast start, migration; the guest must use the asyncify configuration to capture its stack |

Rebuilding state by re-running a syscall log would force the log to understand kernel semantics in order to compact it. Checkpointing avoids that.

Objects that point at the host, such as host files and published ports, are restored through their backends and may fail. Each such failure is reported per object, not as a whole-restore failure.

## Where wasm limits compatibility

These limits are why full compatibility stays a goal rather than a state we reach. Everything below is a known gap and must be listed in the compatibility promise.

| Area | Limit | Behaviour we choose |
| --- | --- | --- |
| Shared file mappings | No MMU, so no coherent `MAP_SHARED` file pages between processes | Impossible; shared file mappings are rejected |
| Shared memory between processes | Wasm modules cannot share part of their memory | Impossible; `MAP_SHARED` anonymous memory stays private to its process, even across fork |
| `MAP_FIXED` | Wasm memory only grows at the end | Rejected for now; may later work over pages `mmap` already handed out |
| `mprotect`, guard pages | Linear memory has no page protection | Impossible; protections are not enforced |
| Fork | No copy-on-write | Asyncify configuration only; memory is copied eagerly |
| Async signals | Guests only stop at interruption checks | Engine interruption checks; latency is bounded by check density |
| `ptrace` | No way to stop and inspect a guest at the instruction level | Exported, returns ENOSYS |
| Executable memory, JIT | Wasm code cannot be generated at runtime inside the guest | `PROT_EXEC` mappings fail |
| Native binaries | Only wasm is executable | `execve` of an ELF file returns ENOEXEC unless a registered format handles it |

## Crate structure

The new crates live under `lib/wasixv2/`, use `wasixv2-` package names, and start unpublished. Only `wasixv2-exec`, `wasixv2-dylink` and `wasixv2` depend on the wasm engine.

| Crate | Directory | Responsibility | Depends on |
| --- | --- | --- | --- |
| `wasixv2` | `api` | Embedding facade: assembles kernel, drivers, network backends, layers and exec; boot configuration | all of the below |
| `wasixv2-abi` | `abi` | Guest-visible constants, errno values, flags and struct layouts for wasm32 | — |
| `wasixv2-codegen` | `codegen` | Build-time generator: syscall table to import shims, decoders, trait, request enum | — |
| `wasixv2-syscalls` | `syscalls` | The `Syscalls` trait, request enum, `Layered` adapter, user-memory types, result and restart types, the return-to-guest interface | abi |
| `wasixv2-trace` | `trace` | Layer: syscall tracing | syscalls |
| `wasixv2-record` | `record` | Layer: record and replay | syscalls |
| `wasixv2-policy` | `policy` | Layer: syscall filtering and policy | syscalls |
| `wasixv2-host` | `host` | What the kernel needs from the host: executor, clocks, timers, entropy | — |
| `wasixv2-wait` | `wait` | Readiness: poll masks, wait queues, poll tables, blocking waits | abi |
| `wasixv2-vfs` | `vfs` | Mount trees, path walk, name cache, permissions, locks, notification, filesystem driver trait | abi |
| `wasixv2-vfs-mem` | `vfs-mem` | Memory filesystem | vfs |
| `wasixv2-vfs-host` | `vfs-host` | Host directory passthrough | vfs |
| `wasixv2-vfs-overlay` | `vfs-overlay` | Overlay filesystem | vfs |
| `wasixv2-vfs-package` | `vfs-package` | Read-only packaged filesystem images | vfs |
| `wasixv2-vnet` | `vnet` | Socket-level network per namespace: port tables, interfaces and routes, in-memory streams and datagram queues, publishing, the backend trait | abi, wait |
| `wasixv2-vnet-host` | `vnet-host` | Backend that carries outbound connections over host sockets and accepts published ports | vnet |
| `wasixv2-vnet-switch` | `vnet-switch` | Socket-level virtual switch that routes connections between machines by address | vnet |
| `wasixv2-kernel` | `kernel` | Tasks, processes, credentials, signals, namespaces, IPC, memory, terminals, pseudo filesystems, socket layer, Unix sockets; implements `Syscalls` | abi, syscalls, host, vfs, vnet, wait |
| `wasixv2-checkpoint` | `checkpoint` | Kernel state checkpoint and restore | kernel |
| `wasixv2-exec` | `exec` | Loading, instances, suspension, return-to-guest, fork, preemption | syscalls, wasmer |
| `wasixv2-dylink` | `dylink` | Side-module loading and linking (wasm extension) | exec, wasmer |

**Dependency rules**

- The kernel never depends on the engine, on layers, or on concrete filesystem drivers and network backends.
- Layers depend only on `wasixv2-syscalls`, so they work over any implementation, including fakes in tests.
- `wasixv2-exec` drives any `Syscalls` implementation through the return-to-guest interface and never reaches into kernel internals.
- Package and registry handling stays outside these crates. It appears only as the package filesystem driver.
- The existing `lib/wasix`, `virtual-fs`, `virtual-net`, `virtual-io` and `journal` crates are not dependencies of anything here.

## Decisions

| Date | Decision | Why |
| --- | --- | --- |
| 2026-10-15 | The executor is reached through `Host::executor`; tokio and the browser are host implementations behind features | Keeps the kernel runtime-agnostic and lets one embedder share an executor between hosts |
| 2026-10-14 | Crates live in this monorepo under lib/wasixv2, named wasixv2-\*, with the facade named wasixv2 | Keeping napi in a separate repo has been painful; the prefix keeps v2 apart from wasixv1 crates |
| 2026-10-14 | The libc port is a separate effort; the first slice is tested with hand-crafted binaries that use no libc | Keeps the kernel work unblocked by libc |
| 2026-10-14 | Keep wasixv1's setjmp-based vfork, with a task switch in the exec layer instead of swapping environments | The guest-side mechanism matches vfork semantics; only the host side was brittle |
| 2026-10-14 | One import module, wasixv2, for everything; kebab-case WIT names; Linux 64 suffixes kept, plain names may use 64-bit types | Simplest naming; the ABI is inspired by riscv32, not copied from it |
| 2026-10-14 | Three build configurations from day one: exception handling with exnref (default), exception handling plus PIC (dynamic linking), asyncify (fork and checkpointing). Stack switching suspends syscalls in all three. | Keeps wasixv1's three real configurations and drops its duplicate legacy-exception variants; stack switching is independent of the exception and asyncify choice |
| 2026-10-14 | Generated code is committed; CI regenerates it and fails on any difference | Reviewable diffs, IDE navigation, no build-time cost |
| 2026-10-14 | Syscall trait methods take a concrete caller type | Simpler than making every method generic |
| 2026-10-14 | The syscall table is written in the component model's format; its code generator is in development | Typed, versionable source of truth; the first implementation slice waits on the generator |
| 2026-10-14 | uname reports machine wasm32 and system name wasixv2 | Matches the target identity: unix, not Linux |
| 2026-10-15 | uname release is the ABI version, 2.0.0; nodename is localhost and domainname `(none)` until UTS namespaces exist | A Linux version would mislead programs that probe Linux features |
| 2026-10-15 | Processes run as uid and gid 1 until credentials exist | Root (0) bypasses permission checks and sends programs down privileged paths |
| 2026-10-15 | The host instantiates and links side modules; libc2 owns policy and runs constructors | Wasm cannot create code from the guest, and threads, startup and per-instance state need the host; everything else stays in libc as on Linux |
| 2026-10-15 | During a link the host only calls wasm-ld's synthesized functions (`__wasm_init_memory`, `__wasm_apply_data_relocs`, `__wasm_init_tls`), never user code or `malloc` | Calls into user code mid-link invite re-entry, deadlocks and unwinding through host frames |
| 2026-10-15 | Side-module data and TLS come from a host-owned arena of kernel pages | A thread stopped while holding the `malloc` lock cannot allocate its own TLS through `malloc` |
| 2026-10-15 | libc2 passes the library search list as data; the host walks it through the `openat` path | Policy stays in libc, the walk is the kernel's, and startup reuses it |
| 2026-10-15 | Stop-the-world for `dlopen` is kernel-internal pending work delivered like a signal, not a signal number | The guest cannot block, ignore or observe it; this is Linux's `task_work` |
| 2026-10-15 | `lseek` and `mmap` take 64-bit offsets directly, keeping riscv32's numbers; riscv32 only has `llseek` and `mmap2` | A wasm import can pass an `i64`; the riscv32 forms only work around 32-bit registers |
| 2026-10-14 | No shared file mappings and no memory protection; mapping support is limited, as in wasixv1's libc | Physically impossible on wasm linear memory |
| 2026-10-14 | The target reports itself as unix but not Linux: target\_family = "unix" and target\_os = "wasixv2" in Rust; clang defines \_\_unix\_\_ but not \_\_linux\_\_ | Low-level code is ported explicitly, as the existing wasixv1 forks already are, and defining \_\_linux\_\_ would not have spared any of them. Code that turns out fully compatible adds a wasixv2 case to its Linux check. |
| 2026-10-14 | Target triple `wasm32-wasixv2` (`wasm32-unknown-wasixv2` in LLVM) | Our own target, independent of other wasm-Linux efforts |
| 2026-10-14 | Page size reported as 64 KiB | It matches the wasm page size |
| 2026-10-14 | wasm32 only, ILP32, generic syscall table in riscv32's shape (time64 only, 64-bit `off_t`, `statx`) | Newest 32-bit Linux ABI, no legacy syscalls, musl support exists |
| 2026-10-14 | Crate names follow our own scheme; Linux is a compatibility target and source of ideas, not of names | Avoid inheriting Linux's naming and structure wholesale |
| 2026-10-14 | No optional syscall imports; libc uses a minimum-kernel policy | The kernel exports the full table, so there is nothing to probe |
| 2026-10-14 | Networking is implemented at the socket layer, with no TCP/IP stack | Guests only observe sockets; in-memory streams and host sockets give Linux semantics and per-namespace port spaces |
| 2026-10-14 | WASI preview 1 dropped unless addable with zero core changes | It must not shape any part of the design |
| 2026-10-14 | Typed import per syscall | Link-time failure on version skew, wasm-level type checks, no dispatch switch |
| 2026-10-14 | Syscall trait with stacked layers over the kernel | One choke point for tracing, recording and policy; kernel testable without wasm |
| 2026-10-14 | Target is full Linux userspace compatibility; current `lib/wasix` is not a reference | Existing design is not worth preserving |
