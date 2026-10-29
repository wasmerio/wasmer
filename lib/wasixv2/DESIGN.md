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
- **Source-level identity:** the target is Linux. Its syscalls, flags and struct layouts are Linux's, and libc2's sysroot ships Linux's UAPI headers (`<linux/*.h>`, `<asm/*.h>`, riscv's, with `<asm/unistd.h>` giving the wasixv2 ABI's numbers). clang defines `__unix__` and `__linux__` (and `__linux`, and `linux` in GNU modes) but not `__gnu_linux__`, since the libc is musl; `__wasixv2__` stays defined for code that needs to tell the target apart. In Rust it is `target_os = "linux"`, `target_env = "musl"`, `target_vendor = "wasixv2"` and `target_family = "unix"` without `"wasm"`, so crates that support Linux and musl (libc, nix, mio, tokio) build unported. The clang predefine lives on the wasix-org LLVM fork's branch `wasixv2-linux-macro` (3dfdb0509); a toolchain downloaded by wasixcc only has it once the fork's owners cut a release that includes it, and until then needs a locally built clang.
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
  - **Init.** The kernel boots with its own init as pid 1, and every program the embedder starts is its child, as on Linux. Init runs no guest code: it ignores SIGCHLD, so the orphans handed to it are reaped as they exit (`forget_original_parent`), and no signal it has no handler for reaches it, SIGKILL included (`SIGNAL_UNKILLABLE`). It is never killed or waited for when the machine shuts down. The embedder can boot without it; programs then start as pid 1 with no parent, and orphans are reaped as they exit. A guest init program can later take its place behind the same option.
- **Credentials.** Real, effective and saved uid and gid, supplementary groups and capabilities. They are checked by the kernel, never by the host.
- **Scheduling attributes.** Policy (`SCHED_OTHER`, `BATCH`, `IDLE`, `FIFO`, `RR`, `DEADLINE`), priorities, nice value, utilization clamps, CPU affinity, timer slack and name are per task, as on Linux, and follow `kernel/sched/syscalls.c`: the same validation, `CAP_SYS_NICE`, `RLIMIT_NICE` and `RLIMIT_RTPRIO` checks, deadline admission, `SCHED_RESET_ON_FORK`, and inheritance on clone and across `execve`. They are advisory: the host schedules guest threads, so none of them changes when or where a guest runs, and real-time policies give no real-time guarantees.
- **Signals.**
  - Pending sets per task and per thread group, masks, dispositions, `sigaltstack`, and queued realtime signals, counted per user against `RLIMIT_SIGPENDING`.
  - Delivered on the return-to-guest path, by the task the signal waits for (`TIF_SIGPENDING`): a thread-directed signal's target, or the one task `complete_signal` picks for a group signal (the leader if it takes it, else the next task that does), so the other tasks neither leave their waits nor take it.
  - Job control as in `kernel/signal.c`: a stop signal's default action stops every task of the thread group on its way back to the guest, where its instance waits until SIGCONT, SIGKILL, an exit or an `execve` ends the stop. The parent hears of stops and continues through SIGCHLD (unless `SA_NOCLDSTOP`) and `waitid`'s `WSTOPPED` and `WCONTINUED`. In an orphaned process group SIGTSTP, SIGTTIN and SIGTTOU do nothing, and a group that an exit orphans with a stopped process in it gets SIGHUP and SIGCONT.
- **Waiting and time.**
  - Wait queues; futexes keyed as Linux's `get_futex_key` keys them: by address space and address, so every task on one memory (threads, a vfork child) meets on them, and in a mapped shared object, unless `FUTEX_PRIVATE_FLAG`, by the object and offset, so processes meet there too. In a shared mapping kept as a copy, a futex works only within its process (see [Mappings without an MMU](#mappings-without-an-mmu)).
  - Wasm's `memory.atomic.wait32`, `wait64` and `notify` are futex operations on the same queues (`AtomicWait`), so the instructions and `futex` wake each other's waiters.
  - Clocks; interval, POSIX and fd-based timers, all driven by host timers, never by polling.
- **Namespaces.** Mount, pid, network, uts, ipc and user namespaces.
- **IPC.** Pipes, Unix sockets, SysV and POSIX message queues, shared memory and semaphores, and event, signal, timer, epoll and inotify descriptors.
- **Memory.** A manager for the guest's linear memory that implements `brk`, anonymous `mmap`, private and shared file mappings, SysV segment attachments and an `mprotect` that binds the kernel's copies but not the guest; see [Mappings without an MMU](#mappings-without-an-mmu) and [Page protection](#page-protection).
- **Terminals.**
  - The line discipline, pseudo-terminals and job control (`SIGTTIN`, `SIGTTOU`, foreground groups) live in the kernel.
  - The host only provides raw byte streams.
  - `kernel/src/tty` follows `drivers/tty`: a terminal is its termios and `n_tty` state, its window size, the session it controls with its foreground group, and its device. A session leader acquires a terminal it opens that no session has, or takes one with `TIOCSCTTY`; children inherit it and `setsid` drops it. A background group that reads its controlling terminal stops with `SIGTTIN`, one that changes it or writes it under `TOSTOP` with `SIGTTOU`; an orphaned group gets `EIO`. A hangup ends the files open on the terminal, and the session leader's exit hangs up a console.
  - The host's console is a terminal device, `/dev/hvc0` (229:0), as a hypervisor's console is a virtual machine's: the embedder hands the kernel the host's side of a terminal in raw mode (`Console`), and the kernel opens it for a new process as getty does for a login. That process leads its session, so it gets the console as its controlling terminal with its group in the foreground, and its group is orphaned as a login shell's is. It is not Linux's `/dev/console`, whose files have no job control.
  - Pseudo-terminals are Linux's Unix98 ptys (`tty/pty.rs`, `drivers/tty/pty.c`): `/dev/ptmx` makes a pair of terminals, each with its own discipline, joined by a buffer per direction (the flip buffer, 8 KiB); the master's termios is raw and its termios, window and job-control `ioctl`s act on the slave. The slaves' nodes are in the pty filesystem (`tty/devpts.rs`, `fs/devpts`), one instance per machine mounted at `/dev/pts`, which shows a slave while its master is open and is the only place a slave opens from. The master's last close hangs the slave up and removes its node; the slave's last close makes the master's reads end with `EIO`. Packet mode, `TIOCSIG` and `TIOCGPTPEER` work as on Linux.
- **Pseudo filesystems.** `/proc`, `/sys` and `/dev` are in the kernel because they expose kernel state. `/sys` has the network interfaces (`class/net`, `devices/virtual/net`) of the network namespace of whoever mounted it, as Linux tags a sysfs mount, so `ip netns exec` sees its namespace's after remounting it.

Open objects follow a three-level split:

1. A **descriptor** is a table slot plus close-on-exec.
2. An **open file** holds the offset and status flags, and is shared by `dup` and `fork`. A call that reads and then moves the offset holds the open file's position lock throughout (`f_pos_lock`), so threads sharing it never use one offset twice.
3. The **object** behind it is an inode, socket, pipe or event source, behind one trait for read, write, poll and ioctl.

### Mappings without an MMU

Wasm cannot make a page of linear memory alias a page of another module's memory, or appear at two addresses. The host can: every linear memory is one host mapping reserved to its maximum that never moves (wasmer's static memory style), so the execution layer can replace a range of it with a `MAP_SHARED` mapping of a host object (`mmap` with `MAP_FIXED`, `wasixv2-pages`). Where the host has such objects (`Host::shared_pages`: a memfd on Linux, an unlinked `shm_open` object on macOS) and the memory can place them (`UserMemory::map_shared`: wasmer's sys backend on Unix), shared memory is shared pages. Everything else is a copy in the process's memory (kernel `mm.rs`, `mm/shared.rs`).

- **Mapped objects.** `MAP_SHARED | MAP_ANONYMOUS` memory gets an object of its own (Linux's `shmem_zero_setup`), a SysV segment is one (`shmem_kernel_file_setup`), and so is each regular file of the memfd and `/dev/shm` tmpfs (`MemFs::with_shared_pages`; the driver hands it out through `Node::shared_pages`), as tmpfs keeps its files in shmem; every mapping and attachment of it, in any process, shows its pages, so a store is seen everywhere at once, and `read` and `write` on such a file reach the same pages. A fork child's memory shows the same objects in the same ranges: the copy of the parent's memory leaves them out (`dup_mmap` copies no pages of a shared mapping), and the child maps them before its low guard is protected, since `MAP_FIXED` resets protections (`SharedMappings`). `munmap`, `shmdt` and `MAP_FIXED` over one give the range zeroed private pages again; `mremap` moves the pages along and grows a mapping only as far as its object reaches; `mremap` with an old length of 0 maps the object a second time; `MADV_DONTNEED` keeps the contents. A mapping that may never be written (`VM_MAYWRITE` clear: a `SHM_RDONLY` attachment, a shared mapping of a file not open for writing or write-sealed) is mapped read-only, as the low guard is protected: a guest store into it traps and the process dies with `SIGSEGV`, the execution layer's one checked write path refuses it (`EFAULT` from a syscall that would store there), and `mprotect(PROT_WRITE)` over it fails with `EACCES`. The execution layer records those ranges per memory, for all its threads, and holds that record while it writes, so no range loses write access under a host copy; a fork child maps them read-only again, and `munmap`, `shmdt` and `MAP_FIXED` give the range writable private pages. Every other range is mapped read-write whatever the protection, since wasm enforces none of its own; none is placed over the low guard or past the object's end, so no host access can raise `SIGBUS`.
- **Files in shared memory.** A file's object is made when it first holds data (an empty file holds no host descriptor); where it cannot be made the file stays in the filesystem's memory and its mappings are copies. Its length is a high-water mark of the file's size and of every mapping's end and never shrinks, so a mapping past the end of the file reads zeroes where Linux raises `SIGBUS`, and `ftruncate` under a mapping frees the pages past the new size by punching them out (`shmem_truncate_range`) instead of shortening the object; growing the file punches out whole pages past the old end that a mapping wrote, so the new bytes read zero. `SEEK_DATA`/`SEEK_HOLE` ask the object, so a load through a mapping makes a page data as on Linux. A mapping of a file not open for writing, or write-sealed, shows the object read-only (see above); a writable one counts against `F_SEAL_WRITE` (`EBUSY`) in every process and part. Only on Linux: a macOS object's size can be set once. Stores through a mapping are not charged to the filesystem's size limit (no page fault to charge them at).
- **Private mappings** are filled from the file when made, and again for pages `mremap` adds and pages `MADV_DONTNEED` drops. They never see later changes to the file, which Linux leaves unspecified for pages a private mapping did not write.
- **Shared mappings kept as copies** (`MAP_SHARED` of a file other than the above, and segment attachments where the host or the memory cannot map the segment: Windows, the JavaScript backend, the kernel's tests) keep a shadow of the object's bytes as of the last synchronisation. Synchronising writes back each byte the memory changed since, then copies in each byte the object changed, so concurrent changes on both sides merge byte by byte and the object wins where both changed one. There, `MAP_SHARED | MAP_ANONYMOUS` memory is ordinary memory of the address space.
- **When a shared file mapping synchronises:** `msync`, `MADV_DONTNEED`, and before and after each data syscall the same process makes on the file (`read`, `write` and their vectored, positioned and copying forms, `ftruncate`, `fallocate`), so one process always sees one copy of the file. It is written back by `munmap`, `mremap` and `MAP_FIXED` over it, and when the address space ends: at `execve`, or by the last task to leave it, as every task does on its way out whatever ended it (`exit`, `exit_group`, a fatal signal: Linux's `exit_mm`), before the process is seen to exit. Mappings belong to the address space: a fork child gets copies (with an attachment of its own per segment), threads and a vfork child share them, and the writeback at the end waits for the last live process on the address space.
- **When a copied segment attachment synchronises:** filled at `shmat`, written back at `shmdt`, `munmap` and the end of the address space, and synchronised by `msync` over it and around `semop`/`semtimedop`, before `msgsnd`/`mq_timedsend` and after `msgrcv`/`mq_timedreceive`. Data handed over under a semaphore or with a message, the usual pattern between processes, therefore arrives as on Linux.
- **Futexes** in a mapped object, unless the call says `FUTEX_PRIVATE_FLAG`, are keyed by the object and the word's offset in it, so process-shared mutexes, condition variables and semaphores work between processes wherever each mapped the object; a contended PI futex's owner finds its word where its own memory maps the object. Any other futex is keyed by address space and address; a shared futex there is the private one, where Linux would keep them apart (`FUT_OFF_MMSHARED`), so the wasm wait and notify instructions, which say neither, meet both kinds.
- **What is not coherent:** in a copied mapping, another process sees a change only once both passed such a point, as does a second mapping in the same process, and futexes work only between threads of one process. Bytes past the end of a file are never written back, and pages past it read as zeroes instead of raising `SIGBUS`; so do the pages an `mremap` grew a mapped object's mapping by past the object.

### Page protection

Wasm has no MMU, so the guest's own loads and stores reach every page of its memory whatever `mmap` and `mprotect` said. The kernel's copies do not: every copy a syscall makes from or to guest memory honours the address space's page map, as Linux's `copy_from_user` and `copy_to_user` fault on a page that is not mapped or whose protection forbids the access. A syscall with a bad pointer therefore fails with `EFAULT` as on Linux, with the partial-copy behaviour each call already has for faults.

- **The rule.** A copy fails with `EFAULT` when it reads or writes a page the address space marks unmapped (pages the allocator handed out and took back: `munmap`, a shrunk `brk`, the gap `MAP_FIXED` past the end of memory leaves) or mapped `PROT_NONE`, or when it writes a page mapped without `PROT_WRITE`. `PROT_WRITE` implies reading, as on x86 and arm64. Everything the kernel never handed out counts as mapped readable and writable: the image's data and stack, and memory the guest grew itself with `memory.grow`, so only ranges the kernel knows to be restricted fault.
- **Where.** The address space (`Mm`) keeps the protections beside its page map and publishes the restricted ranges, lowest first, in a `PageAccess` it owns, whenever the map changes, under the map's lock. The execution layer takes the `PageAccess` of the task an instance runs as (`SharedMappings::page_access`: at instantiation, which covers a fork child and a new image, and when a vfork switches tasks), and every `UserMemory::read` and `write`, and its futex compare-exchange, checks it first. While nothing is restricted the check is one atomic load; otherwise a read lock and a binary search over the ranges.
- **The kernel's own access.** The kernel fills a new file mapping, synchronises copied shared mappings and moves pages for `mremap` whatever their protection, through `UserMemory::force_read` and `force_write` (Linux's `FOLL_FORCE`); those still fault outside the memory, in the low guard and in read-only shared views, which the host itself cannot write.
- **Inheritance.** A fork child's address space starts with its parent's protections (`dup_mmap`); threads and a vfork child share theirs; `execve` starts with none.

### Guard page at 0

Wasmer's engines check memory accesses against the end of linear memory only, so address 0 is ordinary memory and a null pointer dereference reads or writes whatever lives there. The one page protection the host does apply is a guard over the first 64 KiB, the size of Linux's `mmap_min_addr` on x86 and arm64 (`mm/Kconfig`, `security/min_addr.c`). For now the guard is read-only rather than inaccessible:

- **When.** The execution layer protects the guard (`mprotect` `PROT_READ`, Unix hosts) when the instance's ABI opts in and the module's exports show lld's `--stack-first` layout with room to spare: `__stack_pointer` starts at or below `__data_end` and at 128 KiB or above. The guard is then the bottom of the main stack, reached only by a stack about to overflow anyway, and it holds zeros. wasixcc links libc2 programs that way (an 8 MiB stack), and so does Rust's `wasm32-wasixv2`. Data-first modules and modules with smaller stacks run without it.
- **Which ABIs.** The layout alone does not decide: Rust's WASIX v1 programs are linked stack-first too and export `__stack_pointer` at 1 MiB with data above it (coreutils, the condvar and tokio examples), yet v1's libc and guests never promised that low addresses stay unused, and v1 ran them with address 0 writable. So the ABI must opt in as well, through `Abi::low_guard`, which defaults to `false`: only `wasixv2` returns `true`, so WASIX v1, dylink side modules and any other ABI keep address 0 usable. It is a method of the execution layer's `Abi` trait, not a change to any guest ABI.
- **Why reads stay allowed.** clang's wasm32 backend passes a null va_list buffer to a variadic call that has no variadic arguments (`llvm/lib/Target/WebAssembly/WebAssemblyISelLowering.cpp:1454`, `FINode = DAG.getIntPtrConstant(0, DL)`), and musl reads `va_arg` regardless: `fcntl(fd, F_GETFD)`, `__clone` as `posix_spawn` and `popen` call it, `mremap` without a new address, `ioctl`, `semctl`, `prctl`, `syscall`. Rust's calls to variadic C functions go through the same backend. With the guard inaccessible, each of these kills the process (38 of the suite's own tests did), so null reads must stay harmless until the toolchain passes a real buffer.
- **What the guest sees.** A store there traps and kills the process with `SIGSEGV`, through the engine's ordinary out-of-bounds trap; a load there reads zeros. A syscall that writes to a buffer there (`read(fd, NULL, n)`, `clock_gettime(c, NULL)`) fails with `EFAULT`; one that only reads a buffer there succeeds, so `write(fd, NULL, n)` writes zeros. Futexes there fault as in Linux's unmapped pages (`kernel/futex/core.c`): every `futex` operation that reads or changes the word there, and every one without `FUTEX_PRIVATE_FLAG`, whose key needs the page (`get_futex_key`), fails with `EFAULT`, and the robust list walk stops there; a private `FUTEX_WAKE` or `FUTEX_REQUEUE`, keyed by the address alone, wakes nobody, as on Linux. Wasm's atomic wait and notify there behave as the guest's loads do. `MAP_FIXED` and `MREMAP_FIXED` below 64 KiB are `EPERM` for every process, guarded or not, as below the LSM minimum, which no capability lowers; `/proc/sys/vm/mmap_min_addr` reads 65536. A fork child's memory gets the same guard once the parent's is copied into it, and threads share the guard of their process's memory.
- **What the host does.** Every host write to guest memory in the execution layer, and its futex compare-exchange, goes through one checked path that faults inside the guard and inside read-only shared mappings, so the host never writes a protected page; host reads go through as the guest's own loads do. Memories are reserved to their maximum, so growing one never moves the protected pages.
- **Not covered.** Bulk memory instructions (`memory.copy`, `memory.fill`, `memory.init`) that write the guard fault inside the engine's libcall, which is reported as the same out-of-bounds trap. Hosts without `mprotect` (Windows, which needs `VirtualProtect`) run without the guard.
- **Follow-up: an inaccessible guard.** Change the line above in wasix-org's LLVM to pass a pointer to a real (empty) stack slot, which is also an upstream fix, and have the fixed toolchain mark the modules it builds; the execution layer then protects the guard `PROT_NONE`, so null loads fault too, for marked modules only, and modules built before the fix keep the read-only guard.

### SysV IPC and POSIX message queues

Semaphore sets, SysV message queues and POSIX message queues live entirely in the kernel (`ipc/`), so they work between any processes of the machine as on Linux: keys and `IPC_PRIVATE`, ids of `seq * 32768 + index`, `ipc_perm` checks with `CAP_IPC_OWNER`, `IPC_STAT`/`IPC_SET`/`IPC_RMID`/`IPC_INFO` and the `*_STAT`/`*_INFO` variants with Linux's limits, all-or-nothing `semop` with `SEM_UNDO` applied at process exit, and `msgrcv`'s type selection. POSIX queues are open files that poll and read as Linux's `mqueue` files do; their names live in a kernel table rather than a mounted `mqueue` filesystem, and each queue's inode sits on an internal `mqueue` mount, which names its descriptors `/<name>`. `mq_notify` delivers `SIGEV_SIGNAL` with `SI_MESGQ`, and `SIGEV_THREAD` as Linux does, by sending musl's cookie over the netlink socket it names. All of them belong to the IPC namespace: one made by `CLONE_NEWIPC` starts empty, with its own keys, ids and queue names.

### Readiness and wait queues

The crate `wasixv2-wait` holds what the VFS, the network and the kernel all need for readiness. It is the counterpart of Linux's `wait_queue_head_t`, `poll_table` and `__poll_t`.

- **`PollMask`**: the `EPOLL*` bits, which equal the `POLL*` bits on the generic architecture.
- **`WaitQueue`**: owned by an object, which calls `wake(events)` whenever its state changes, from any thread. An empty key means "anything".
  - `add(Arc<dyn WaitEntry>)` returns a guard that unregisters on drop.
  - Exclusive entries queue last. A wake stops at the first exclusive entry that takes it; a task that is already woken does not take another.
  - Entries run under the queue's lock, as Linux's wake functions do, so an entry whose registration was dropped never runs again.
- **`Pollable::poll(Option<&mut PollTable>) -> PollMask`**: Linux's `f_op->poll`. It registers on the object's queues through the table and returns the current mask without blocking.
- **Waiting** goes through Rust wakers, so it works on any executor:
  - `Waiter` is one task's entry and can sit on many queues. Its `handle` wakes it directly, for waits that keep their own record of sleepers, as futexes do (`futex_q`).
  - `PollWaiter` is `poll_wqueues`, used by poll and select.
  - `wait_event`, `wait_event_timeout` and `wait_event_exclusive` are cancellation-safe. An exclusive wakeup that a cancelled waiter took is passed on.
  - Each takes an `Interrupt` hook, the kernel's `signal_pending`.

### The object trait

`FileObject` (kernel `file.rs`) extends `Pollable` with:

- `read` and `write`, which take an `Io` holding the open file's flags as the call began plus the interrupt hook. They block asynchronously unless `O_NONBLOCK` is set, in which case they return `EAGAIN`.
- `read_to` (`read_iter`): a read that copies to the caller's buffers itself through a `ReadSink`. Streams (pipes and FIFOs, host stdin, Unix and TCP stream sockets) look at their data under a per-object I/O lock (`pipe->mutex`, `u->iolock`, `lock_sock`), copy it out and take it only once all of it arrived, so a read whose buffer faults is `EFAULT` and leaves the data queued, as Linux takes a pipe buffer or an skb only once it was copied. The lock is never held while waiting for data. Other objects take their data first, and lose what a fault kept from the reader, as eventfd, timerfd, signalfd, inotify and datagram sockets do on Linux.
- `ioctl`, which defaults to `ENOTTY`. The kernel handles `FIOCLEX`, `FIONCLEX`, `FIONBIO`, and `FIONREAD` on regular files, before calling the object, and `FICLONE`, `FICLONERANGE` and `FIDEDUPERANGE` for every file: they make `ioctl_file_clone`'s and `vfs_dedupe_file_range`'s checks and then fail with `EOPNOTSUPP`, since no filesystem here shares extents.
- `can_poll`, which is `file_can_poll`.
- `fills_reads`, for objects whose reads fill the buffer, as the memory and random devices' do (`read_iter_zero`): the kernel reads them in steps until the buffer is full, as it reads files, and stops early at a signal. A read of any other object returns what one read of it gives.

Dropping the object is Linux's `release`. Inode-backed open files poll as `DEFAULT_POLLMASK` and are `EPERM` for epoll.

### Pipes and splice

A pipe holds bytes rather than page buffers (`kernel/src/pipe.rs`), so `splice`, `tee` and `vmsplice` (`fs/splice.rs`) copy where Linux moves or lends pages; what they take, give and return follows `fs/splice.c`.

- **Consumers** (reads, `splice` out of a pipe, `vmsplice` to the user) wait for data without a lock, then hold the pipe's I/O lock (`pipe->mutex`) from looking at the data until they consumed what their output took. A `splice` to a file or socket holds it across that write, as Linux holds the pipe lock across `splice_write`.
- **Producers** append under the pipe's state lock. A `splice` or `vmsplice` into a pipe first reserves the room it will fill, which writers then do not see, and fills it once its input is read, as Linux holds the pipe lock across `splice_read`.
- **Pipe to pipe:** `splice` and `tee` lock both pipes' states in address order (`pipe_double_lock`); `splice` takes the input's I/O lock first. Lock order: I/O lock, then the pipe states.
- **Hints:** `SPLICE_F_MOVE` and `SPLICE_F_GIFT` are ignored, as Linux may; `SPLICE_F_MORE` is a hint.
- **Not yet:** host stdio streams look like pipes but are not, so they are not spliced as pipes.

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
  - Epolls nest up to `EP_MAX_NESTS`, and a nesting that would create a loop is `ELOOP`. One kernel-wide lock holds the loop check and the insert together (`epnested_mutex`).
  - `epoll_wait` is never restarted.
- **Not yet:**
  - `reverse_path_check`'s wakeup-path limits.
  - `max_user_watches`.

## Filesystems

There is one VFS. It owns all path and permission semantics, and filesystem drivers only store data.

**The VFS owns:**

- A mount tree per mount namespace: bind mounts, read-only mounts, overmounting, and propagation between shared and slave mounts.
- Path walking: symlinks, `..`, and the `openat2` resolve flags.
- An inode cache and a name cache.
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
| Memory | tmpfs-style, the default writable root; `size=` and `nr_inodes=` limits (default half of the machine's memory) with `ENOSPC` |
| Host | Passthrough to a host directory |
| Overlay | Writable upper layer over read-only lower layers |
| Package | Read-only view of a packaged filesystem image |
| Kernel pseudo | `/proc`, the character devices, and pipefs, sockfs and anon_inodefs; `/sys` and `/dev/pts` later (in the kernel crate) |

Mounts are the only way to compose filesystems. Overlay is a filesystem, not a second composition mechanism.

### The VFS and driver split

`wasixv2-vfs` decides every path, permission and ownership question, following Linux's `fs/namei.c`, `fs/open.c`, `fs/attr.c`, `fs/utimes.c` and `fs/xattr.c`, including their error precedence. The kernel only decodes syscall arguments, keeps per-process state (root, working directory, umask, credentials) and calls `Vfs`. Drivers never see paths, `.`, `..`, symlink resolution, credentials or permission checks: they get single names in a directory they already returned, and objects of their own filesystem.

```rust
#[async_trait(?Send)]
pub trait Filesystem: Send + Sync {
    fn info(&self) -> FsInfo;                       // statfs magic, name_max, read_only, dev, has_btime, stable
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
    async fn create_open(&self, name: &[u8], new: &NewNode<'_>, options: OpenOptions)
        -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)>;                                  // O_CREAT that creates
    async fn create_unnamed(&self, new: &NewNode<'_>, options: OpenOptions)
        -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)>;                                  // O_TMPFILE
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
    async fn set_size(&self, size: u64) -> FsResult<()>;                // ftruncate
    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64>;
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()>;  // no . or ..
}
```

- **Identity.** `Node::ino` is the object's identity within its filesystem: the VFS keeps one inode per number, so two nodes with one number must be one object, and a driver spanning several devices folds the device into the number. `Attr::dev` names a driver device other than the filesystem's; the VFS shows each as an anonymous device of its own, never the driver's number.
- **Timestamps.** Drivers keep the times their own operations imply (a write stamps `mtime` and `ctime`, a new entry stamps the parent), as Linux filesystems do in their inode operations; explicit changes arrive through `setattr`.
- **Directory cookies** resume a read while entries come and go, and stay below 2^31 so a 32-bit `telldir` holds them. The VFS adds `.` and `..` at positions 0 and 1, and a position past the end, as tmpfs's `simple_offset` readdir.
- **Creation opens.** An `open` that creates its file gets the access it asked for whatever the new file's mode (`FMODE_CREATED`), so the driver creates and opens in one step (`create_open`, `create_unnamed`, after `->atomic_open` and `->tmpfile`) and hands back the handle. `ftruncate` goes through the open file's handle (`set_size`, `ATTR_FILE`), so it needs the descriptor's write access and never the mode bits.
- **Open files.** `vfs::File` is the filesystem part of an open file description (path, handle, lock owner id); the kernel's `OpenFile` holds it as `Backing::Inode` with the position and status flags. Dropping the last reference is the last close. Every other open file (`Backing::Stream`) also has a path, as Linux's `f_path`: a FIFO or device node it was opened through, or an inode on a filesystem mounted nowhere (below).
- **Devices.** The VFS opens FIFOs and device nodes without a handle; the kernel attaches the pipe bound to the inode (`fifo_open`) or the character device driver the node's number names (`chrdev_open`), and `ENXIO` when there is none. Sockets are `ENXIO` (`sock_no_open`).

### Mounts, names and path walking

- **Mount tree.** A `Mount` is a superblock plus the dentry its tree starts at (a filesystem root, or any directory for a bind mount) and per-mount flags (read-only, nosuid, nodev, noexec, atime, nosymfollow), which a remount changes. A mountpoint `(mount, dentry)` holds one mount; mounting on a covered place mounts on the root of the topmost mount there (`where_to_mount`). The embedder builds the initial namespace's tree with `Vfs::mount` and `Vfs::bind_mount`.
- **Mount namespaces.** One table holds every mount of the VFS, keyed by the mount and dentry it is attached on, with each mount's namespace beside it; mounts are never shared between namespaces (`copy_mnt_ns` copies them), so a walk crosses mounts without knowing its namespace. `Vfs::do_mount` (new, bind, recursive bind, move, remount, propagation), `do_umount`, `pivot_root` and `mount_setattr` change only mounts of the caller's namespace (`check_mnt`); the kernel resolves their paths and checks `CAP_SYS_ADMIN`. A mount is busy while anything but the table and the caller holds it (an open file, a working directory, a mount on it), as Linux's reference count. Dropping a namespace unmounts its tree; what still holds a mount keeps it, detached. Removing a name that another namespace has a mount on unmounts that mount (`detach_mounts`); in the caller's own namespace it is `EBUSY`. A file open for writing holds write access on its mount (`mnt_writers`), so neither the mount nor its filesystem becomes read-only meanwhile (`EBUSY`).
- **Detached trees.** `Vfs::open_tree` copies a tree and `Vfs::create_mount` makes a new mount into an anonymous namespace of its own (`is_anon`, with the namespace it came from), held by an `O_PATH` file at its root, whose last close unmounts what is still there (`dissolve_on_fput`). `Vfs::move_mount` attaches such a root with its tree into the caller's namespace, or into another detached tree from it, and moves mounts of the caller's namespace; with `beneath` the tree goes under the topmost mount on the target, which then sits on the tree (`MOVE_MOUNT_BENEATH`). Propagation does not reach detached trees.
- **Propagation.** As `fs/pnode.c`, per peer group: shared mounts carry a group id, slaves the id of the group they receive from. A mount attached on a shared mount is copied onto every peer and, through the groups, every slave, their peers and slaves (`propagate_mnt`), tucked under what a receiver already has there; unmounting a tree unmounts the copies of it that nothing staying is mounted on, and a mount on the root of a copy that goes takes the copy's place (`propagate_umount` of Linux 6.15 on, with the same chains for `propagate_mount_busy`). A mount that leaves its group hands the group's slaves to its master when it was the last member. `mountinfo` shows `shared:N`, `master:N` and `propagate_from:N`.
- **The mount syscalls.** `kernel/src/fs/mount.rs` decodes `mount` (`path_mount`'s dispatch, `MS_*` to per-mount flags, the options every filesystem takes such as `ro`), `umount2`, `pivot_root` (then moving every root and working directory at the old root, `chroot_fs_refs`), `mount_setattr`, `open_tree`, `move_mount`, `statmount` and `listmount` (by the unique mount ids `statx(STATX_MNT_ID_UNIQUE)` reports), and `kernel/src/fs/fsopen.rs` the filesystem contexts of `fsopen`, `fsconfig`, `fsmount` and `fspick` (anonymous files; parameters checked one by one, the instance made at `FSCONFIG_CMD_CREATE`), and checks `CAP_SYS_ADMIN` where Linux does. New mounts are instances of the filesystem types the kernel knows (`Kernel::register_filesystem`, listed by `/proc/filesystems`): `proc` is the kernel's own, and the facade registers `tmpfs` (a memory filesystem in host shared memory, with shmem's `size=`, `nr_blocks=`, `nr_inodes=`, `mode=`, `uid=` and `gid=`, whose limits a remount changes, also of `/tmp`, `/dev` and `/dev/shm`) and `devtmpfs` (the instance at `/dev`). Mounting a filesystem that is mounted already uses its superblock and root again (`sget`). `mountinfo`, `/proc/<pid>/mounts` and `statmount` show a filesystem's own options (`Filesystem::show_options`; tmpfs's limits, mode and owner that are not the defaults).
- **Inodes.** A VFS `Inode` wraps the driver's node and is interned per (superblock, inode number) while anything holds it, so hard links and repeated lookups share one. It carries what Linux keeps on `struct inode`: the directory lock, the removed-directory flag (`S_DEAD`), the `O_TMPFILE` linkable flag (`I_LINKABLE`), a directory's single dentry, and the seals of a tmpfs file (`F_ADD_SEALS`), which the VFS enforces on every write, size change, allocation and mode change, under the inode lock while they can still change, with the count of writable shared mappings (`i_mmap_writable`) that makes adding `F_SEAL_WRITE` fail with `EBUSY`. FIFO pipes, bound Unix sockets, inotify marks and write denial for running executables belong here too when they arrive.
- **Dentries.** The VFS keeps the tree of names it has seen, for `..`, `getcwd`, mountpoints and the rename ancestry check. Only dentries something holds stay alive. A lookup asks the driver and reuses the cached dentry only when it names the same object, so names that change behind the VFS (on a host directory) are never answered from a stale cache (`d_revalidate` that always revalidates). A filesystem whose names change only through the VFS (`FsInfo::stable`: memory, package, and overlays of them) answers a cached name without asking the driver, as Linux's dcache does for filesystems without `d_revalidate`. A directory has at most one dentry: found under a new name, its dentry moves there (`d_splice_alias`), and a name the driver no longer has marks its dentry deleted. There is no LRU of unused dentries and no negative or attribute cache yet.
- **Locking.** Each inode has an async directory lock (`i_rwsem`, always exclusive). A walk holds it for one lookup and its cache update. `create`, `link`, `unlink`, `rmdir`, `rename` and `O_CREAT` hold the parent from the final lookup through the driver call and the cache update, so their checks stay true and `O_CREAT|O_EXCL` is atomic whatever the driver does. `rmdir` and a rename over a directory also lock the victim and mark it removed; nothing is created in a removed directory (`ENOENT`). Cross-directory renames take the superblock's rename lock, then both parents, the ancestor first and otherwise in a fixed order (`lock_rename`). Order is always parent before child. `getcwd` and `d_path` read the name tree without a rename seqlock, so a path built during a concurrent cross-directory rename can mix old and new names.
- **Cancellation.** A VFS call dropped after its driver call but before its cache update leaves a stale name until the next lookup corrects it. Calls are only dropped when their task is torn down; blocking points use the `Interrupt` hook instead.
- **Walk.** `link_path_walk` semantics: search permission on every directory, symlinks followed in the middle and at the end unless `NOFOLLOW`, at most 40 per walk (`ELOOP`), trailing slashes demand a directory and force following, `..` stops at the process root and climbs from a mount root to its mountpoint, and the `openat2` scopes `RESOLVE_BENEATH`, `IN_ROOT`, `NO_XDEV`, `NO_SYMLINKS` and `NO_MAGICLINKS` (`RESOLVE_CACHED` resolves normally). A scoped walk fails a `..` with `EAGAIN` if any rename or mount changed the tree since it started, since `..` might then have left the scope (`rename_lock` and `mount_lock` in `handle_dots`).
- **Magic links.** Following a symlink asks the node's `get_link(cx)`, which by default is `readlink`'s body. A pseudo filesystem may answer per caller (`Ctx::task` names the calling task, for `/proc/self`) or with `Link::Jump(path)`: an object itself, not a string, as Linux's `nd_jump_link`. The walk continues from that path, so `/proc/self/fd/3` opens what descriptor 3 has open (a file unlinked since, an `O_PATH` file, a pipe) and `/proc/self/cwd/x` walks on from the working directory. A walk that ends on a jump has no last name (`LAST_BIND`): `open` with `O_CREAT` opens the object, `mkdir` is `EEXIST`, `unlink` `EISDIR`. Jumps are `ELOOP` under `RESOLVE_NO_MAGICLINKS` and `EXDEV` in scoped walks (`RESOLVE_BENEATH`, `IN_ROOT`) or across mounts under `NO_XDEV`. `readlink` of a magic link is its object's `d_path` from the caller's root.
- **Objects without a name.** `Vfs::kern_mount` mounts a filesystem outside the tree (`kern_mount`), and `Mount::pseudo_path` names one of its objects without a directory (`d_alloc_pseudo`). `d_path` shows such a path as the filesystem's `dname` says (`d_dname`): `pipe:[ino]`, `socket:[ino]`, `anon_inode:[eventfd]`, or else as `/<name> (deleted)` (`simple_dname`). The kernel's shmem filesystem for `memfd_create` is such a mount: a tmpfs the embedder supplies with `Kernel::with_shm`.

### Kernel filesystems

The kernel implements three kinds of filesystem itself, because they expose its state:

- **pipefs, sockfs and anon_inodefs** (`kernel/src/pseudo.rs`), mounted nowhere. Every pipe and socket gets an inode of its own (FIFO `0600`, socket `0777`), so `fstat` reports a real inode number and `/proc/<pid>/fd` names it. A pipe is bound to its inode as a FIFO's is, so opening its fd link attaches another end to the same pipe. Event, timer, epoll, inotify and pid descriptors share one anonymous inode, named per class; it reports no file type in `st_mode` (`anon_inode_getattr`), refuses `setattr` and does not open again (`no_open`). The host's stdio streams look like pipes: each has a pipefs inode bound to the stream, so `/dev/stdout` reaches the same stream.
- **Character devices** (`kernel/src/chrdev.rs`): `null`, `zero`, `full` (major 1, `drivers/char/mem.c`), `random` and `urandom` (1:8, 1:9, reading the host's entropy without blocking) `tty` (5:0, the caller's controlling terminal, `ENXIO` without one) and the console `hvc0` (229:0, see Terminals, owned by the machine's user, `0620`, as `login` leaves a terminal). Devices are positional: `pread` works and `lseek` lands at 0 (`null_lseek`) or stays (`noop_llseek`). The nodes live on an ordinary filesystem, as devtmpfs is a tmpfs: `Kernel::create_device_nodes` makes them, owned by root, `0666`.
- **procfs** (`kernel/src/procfs`), from `Kernel::procfs`. Nothing is stored: each node is computed from the process tree, the task table and the VFS when looked up, a directory is listed afresh on every read (`proc_readdir`), and a file's text is produced at open and read from that snapshot, as `seq_file`'s first read. It holds the kernel's state weakly. Implemented: `self`, `thread-self`, `mounts`, `cpuinfo`, `meminfo`, `uptime`, `loadavg`, `stat`, `version`, `filesystems`, `cmdline`, `sys/kernel/{hostname,domainname,ostype,osrelease,version,pid_max}` (the names of the reader's UTS namespace, as for `version`), `sys/vm/overcommit_memory`, `sys/fs/{file-max,nr_open,pipe-max-size}`, `net` (a link to `self/net`), and per process and per thread (`task/<tid>`) `fd/`, `fdinfo/`, `ns/`, `net/{tcp,tcp6,udp,udp6,unix,dev}` from the task's network namespace's port table and interfaces, `exe`, `cwd`, `root`, `cmdline`, `environ`, `comm` (writable), `stat`, `statm`, `status`, `maps`, `limits`, `mounts`, `mountinfo`. `fd/<n>`, `exe`, `cwd` and `root` are magic links. `maps` shows the image's pages (`[image]`: data, stack and heap below the allocator's first page), `[heap]` for the break and anonymous runs; `exe` is the file `execve` loaded, and missing (`ENOENT`) for a process the embedder started from bytes.

The facade lays out the default root as a small Linux system (`wasixv2::default_root`): `/bin`, `/usr/bin`, `/usr/lib`, `/root`, `/home/user`, and `/etc/{passwd,group,hostname,hosts,resolv.conf}` naming root, the user processes run as and, with Debian's names and ids, the unprivileged `nobody` (65534) and the groups `users` (100) and `nogroup` (65534). On it, or on an embedder's root (`Machine::with_root`), it mounts procfs at `/proc` (`nosuid,nodev,noexec`), a memory filesystem at `/dev` with the device nodes and `fd`, `stdin`, `stdout` and `stderr` links into `/proc/self/fd`, and sticky world-writable memory filesystems at `/dev/shm` (POSIX shared memory) and `/tmp`.

### Permissions

`generic_permission` over the owner, group (with supplementary groups) and other bits, plus the DAC-override capabilities; sticky directories; set-gid inheritance and the umask at creation; `chmod`, `chown` and `utimensat` ownership rules. Processes run as uid and gid 1 without capabilities. Read-only filesystems and mounts give `EROFS` where Linux does.

### Locks

`LockManager` holds `flock` locks (owned by an open file), POSIX record locks (owned by a process, dropped when it closes any descriptor of the file) and OFD record locks (owned by an open file). Record locks of both kinds conflict with each other and split and merge like Linux's; `flock` locks are separate. Blocking requests wait for any lock change, and `F_SETLKW` detects deadlock cycles between processes (`EDEADLK`). A signal ends the wait, and the call restarts unless the handler lacks `SA_RESTART`.

### Change notification

Every change the VFS makes is reported to one `FsWatcher` as an `FsEvent` (create, delete, move, modify, attrib, access, open, close), keyed by object identity (superblock id, inode number); the kernel's inotify instances hang off this hook. While the watcher watches nothing, the VFS does not build the events of data access at all. Changes made on the host side of a host directory are not seen.

### Host directories

`wasixv2-vfs-host` (Linux hosts only for now) holds every object as an `O_PATH` descriptor, so a file removed on the host stays usable through a guest that holds it, and resolves names with `openat(dir, name, O_PATH | O_NOFOLLOW)`, one component at a time and never `..`, so neither symlinks nor `..` can leave the mapped directory: the VFS interprets symlink targets inside the guest tree. Operations that need more than an `O_PATH` descriptor go through `/proc/self/fd/N`, which names the held inode exactly, as libfuse's `passthrough_ll` does. Reopening that way makes the host check the mode bits again, so an open file's handle keeps the descriptor its open returned and does all its I/O, `ftruncate` and `O_APPEND` writes (`RWF_APPEND`) through it, and a file the guest creates keeps its creating descriptor (`O_CREAT|O_EXCL`, `O_TMPFILE`): a file created read-only stays writable through the open that made it, as on Linux. Blocking host calls run on the `Host` executor's blocking pool. Files the host user owns appear as owned by the guest user, and the reverse. Inode numbers are the host's, except on host mounts inside the directory, whose device index is folded into the high bits (overlayfs's `xino`) so numbers stay unique, and numbers too large to fold, which get one from a counter; those objects report their host device, which the VFS shows as an anonymous device of its own.

### Package filesystems

`wasixv2-vfs-package` mounts one volume of a webc v3 package read-only, like squashfs or erofs: `PackageFs::new(&OwnedReader, "atom")` gives a `Filesystem` to pass to `Vfs::mount`. Only the v3 reader (`webc::v3::read`) is used, never the version-agnostic container API.

- **Untrusted images.** Mounting decodes every directory once and refuses (`PackageFsError::Malformed`) an image whose entries do not decode or point outside the volume, whose names are not file names (empty, `.`, `..`, with `/` or NUL, longer than `NAME_MAX`, twice in a directory), or whose directories are shared or loop (more entries than the volume has room for). webc 12 panics on some malformed entries instead of failing; the driver contains that where panics unwind, and webc must check them itself for `panic = "abort"` builds.
- **Lazy and zero-copy.** A directory's entries are parsed on its first lookup or read into a name-ordered table (v3 writes them sorted) that every node for it shares; lookups binary-search it. File data is a slice of the container's `OwnedBuffer` (mmap or `Bytes`); reads copy only into the caller's buffer. The tables borrow the volume header for `'static` while every holder keeps the volume alive.
- **Embedding.** `Machine::mount_package_volume` mounts a volume either read-only or copy-on-write, as the lower layer of an overlay with an in-memory upper layer. It uses `PackageFs::with_owner` so the guest user owns every object and can change copied-up files. `wasmer run` uses this for every package: it mounts the `fs` volumes of the whole dependency tree copy-on-write, as v1 does, the root package's over its dependencies'. Nothing can be mounted over the root, so a volume at `/` (and a host directory mapped to `/`) becomes a read-only lower layer of the root filesystem instead, under the in-memory layer that holds `/tmp`, `/etc` and every change (`Machine::mount_root_layers`, which stacks all of them, commands included, with one boot of the kernel); the root can only change before anything is mounted or run. A volume becomes a filesystem once (`Machine::package_layer`, which checks every directory), whose files every mount of it shares (`Machine::mount_layer`). webc v2 files are migrated to v3 in memory (`webc::migration::v2_to_v3`); `PackageFs` stays v3-only.
- **Metadata.** Everything is owned by root unless the embedder picks an owner: files and directories `0755` (directories with `2 + subdirectories` links), symlinks `0777`. Files are executable because v3 records no mode bits and packages carry programs and scripts. Timestamps are the epoch, because the v3 reader does not expose the stored modification time. The root's inode number is 1; the entries of a directory take consecutive numbers from a per-volume counter the first time the directory is parsed, so no two objects share a number (an inode number is the object's identity in the VFS).
- **Package commands.** `PackageFs::from_files` builds the same read-only tree from (relative path, `OwnedBuffer`) pairs, with the directories the paths imply. `Machine::install_commands` uses it to put each command's atom at `/bin/<name>` and `/usr/bin/<name>`: a root lower layer over the package volumes, so a command hides a volume file of its name (v1 wrote commands into the root after merging volumes), shared with the atom buffer, first name winning. `wasmer run` installs the commands of the whole tree in the resolver's order (the root's, then its dependencies' in topological order, each name taken by the first package that has it) and starts the entry command by executing `/bin/<name>`, so the kernel never knows commands from other files.
- **Read-only.** `FsInfo::read_only` makes the VFS answer `EROFS` and `statfs` report `ST_RDONLY`; the driver also returns `EROFS` for every mutation. `f_type` is `0x77656263` ("webc"), blocks are the volume's size and none are free. There are no extended attributes (`ENODATA`, empty list) and no holes. v3 symlinks are served as symlinks.

### Overlay filesystem

`wasixv2-vfs-overlay` follows Linux overlayfs. `OverlayFs::new(upper, lowers)` stacks an optional writable upper layer over read-only lower layers, topmost first. Every layer can be any driver. Without an upper layer, the overlay is read-only and every change returns `EROFS`.

- **Lookup.** A name resolves top-down through the layers. A non-directory hides everything below it. Directories with the same name merge until one of them is opaque. A whiteout hides the name in all lower layers.
- **On-disk format.** This matches Linux, so a host upper directory stays compatible with the kernel's overlayfs.
  - A whiteout is a 0/0 character device. Where the upper driver cannot create one (for example a host directory without `CAP_MKNOD`), it is an empty file with an `overlay.whiteout` xattr, and its directory is marked `overlay.opaque=x` (Linux 6.7's xwhiteouts).
  - An opaque directory has `overlay.opaque=y`.
  - Markers are written as `trusted.overlay.*`, falling back to `user.overlay.*`. Both prefixes are read, and both are hidden from programs.
- **Copy-up.** Copy-up runs before any of these touches a lower object: open for write, `setattr` (including truncate), xattr changes, `link`, and any entry change in a lower directory. Ancestors are copied up first. Data is copied with holes preserved, along with mode, ownership, xattrs, and atime and mtime. The parent keeps its times.
  - There is no workdir. A regular file is filled while unnamed (`O_TMPFILE`) and linked in only when complete, where the upper driver supports that. A copy is owner-writable until it is complete and gets its mode last, so copying works whatever the mode (a host upper checks it). A whiteout that replaces an upper entry is made under a temporary name first and renamed over the entry, so failing to make one removes nothing.
  - An open file whose name was removed is copied up unnamed and stays unnamed (`d_unhashed`). An open with `O_TRUNC` copies no data.
  - A file opened read-only before its copy-up switches to the copy on its next access. This matches Linux 4.19 and later.
- **Removal.** `unlink` and `rmdir` leave a whiteout wherever a lower layer still has the name. `rmdir` of a merged directory requires every layer to be empty apart from whiteouts. Creating a directory over a whiteout makes the new directory opaque.
- **Rename.** A directory that has lower parts returns `EXDEV`, as Linux does without `redirect_dir`. Programs then fall back to copying. Non-directories are copied up and then renamed. A directory moved onto a name that a lower layer has is made opaque. `RENAME_WHITEOUT` returns `EINVAL`.
- **Inode numbers.** Inode numbers follow `xino`: a real inode number below 2^56 is combined with its layer index in the top byte. Larger numbers get a counter instead.
  - A copied-up object keeps the number of its lower origin. Linux records the origin in an xattr; here it is remembered in memory for the life of the overlay.
  - A merged directory takes its number from its topmost lower directory, which no other overlay object can reach, since directories have no hard links and merged directories cannot move. `readdir` reports the same numbers as `stat`.
  - Hard links between lower files are separate overlay objects, broken by copy-up as Linux does with `index=off`. Because an inode number is the object's identity in the VFS, each such link (a lower non-directory with more than one link) takes a counter number of its own while it is in use, instead of the shared real one; for these `readdir` still reports the layer's number.
  - The layer byte keeps upper objects, lower objects and counter numbers apart, so a copied-up object keeping its lower number cannot collide with anything that lives only in the upper layer.
- **readdir.** A directory handle reads the merged listing when reading starts at cookie 0. Cookies index into that snapshot, as in Linux's `ovl_dir_cache`.
- **Locking.** One overlay-wide lock serialises namespace changes and copy-ups for metadata changes. Opening a file for writing copies its data into an unnamed file without it and takes it only to link the copy in; a per-object lock makes one copy. Data I/O and metadata changes of objects already in the upper layer do not take it.
- **Reporting.** A merged directory reports `nlink` 1. `statfs` comes from the upper layer, or from the top lower layer when there is no upper.

## Networking

The kernel implements networking at the socket layer and has no TCP/IP stack. A guest can only observe sockets (ports, `bind` conflicts, accept queues, addresses, options, errno), and in-memory streams plus host sockets cover all of that.

- **Port spaces** belong to network namespaces and live in kernel tables. Ephemeral ranges, `EADDRINUSE`, `SO_REUSEADDR`, `SO_REUSEPORT` and the low-port capability check are decided against guest state. Ports the host has bound never change what a guest `bind` returns.
- **Inside a namespace:**
  - A `connect` to a local listener queues on that listener.
  - `accept` pairs the two sockets with an in-memory stream.
  - UDP datagrams move directly between socket queues.
  - Nothing is turned into packets.
- **Interfaces and routes** are kernel tables served over netlink, so `ip addr`, `getifaddrs` and routing lookups work. No frames sit behind them.
  - They change as Linux's do (`net/ipv4/devinet.c`, `fib_*.c`, `net/ipv6/addrconf.c`, `route.c`): an address brings its prefix route, an IPv4 address in another's subnet is its secondary and goes with it, a gateway must be reachable through a connected route, `down` drops an interface's IPv4 routes and IPv6 addresses, `up` gives it its link-local address. Duplicate address detection succeeds at once, and there is no neighbour discovery.
  - The routing table decides what is local, the source address of an unbound socket, and where traffic leaves: through the uplink to the backend, across a veth, or nowhere (a dummy, a veth without carrier). A lookup without a route is `ENETUNREACH`; unreachable, blackhole and prohibit routes fail as Linux's do.
  - Changes take one lock for every namespace (`rtnl_lock`), so moving an interface between namespaces is one change.
- **Multicast:** sockets join groups on an interface (`IP_ADD_MEMBERSHIP`, `MCAST_JOIN_GROUP`, `IPV6_ADD_MEMBERSHIP`), which puts the interface in the group while a membership holds it; an up interface is also in the groups the kernel joins itself (all-hosts, all-nodes, solicited-node), and `/proc/net/igmp` and `igmp6` list them all. A datagram to a group leaves by `IP_MULTICAST_IF` (or the control message's interface, the socket's device, the source address's interface, the route), loops back to the sender's namespace with `IP_MULTICAST_LOOP` if that interface is in the group, and crosses a veth while its hop limit is above 0 if the peer's end is in the group; loopback receives what it sends. In each namespace it reaches, every socket bound to the port that allows the group (a membership, else `IP_MULTICAST_ALL`) gets a copy. There are no IGMP or MLD messages, no multicast to the host, and no source filters (`IP_ADD_SOURCE_MEMBERSHIP` and the like are `ENOPROTOOPT`).
- **Interfaces a guest makes:** `dummy` interfaces and `veth` pairs (`ip link add`), which can move to another namespace (`IFLA_NET_NS_PID`, `IFLA_NET_NS_FD`), arriving down and without addresses. A veth carries what the other namespaces' sockets see: a `connect` or datagram routed out one end reaches the namespace at the other end if that namespace holds the destination address, as loopback within one namespace does. There is no forwarding: a destination the peer does not hold is unreachable, and a stream `connect` fails with `EHOSTUNREACH` at once, where Linux fails after neighbour resolution gives up; datagrams are lost. Deleting one end deletes the other, and a namespace that exits takes the peers of its ends with it.
- **The uplink and loopback** are not a request's to make: deleting them is `EOPNOTSUPP` and moving them `EINVAL` (`NETIF_F_NETNS_LOCAL`). The uplink's addresses, routes, name and MTU can change, but only in what the guest sees: the host's interfaces never change, and its traffic still leaves through the backend.
- **Namespaces:** `unshare` of the network namespace gives a fresh port space with only loopback.
- **Sysctls:** `/proc/sys/net` shows the reader's network namespace. A driver learns the task only when it is opened, so its nodes are the same for every namespace, interface directories go by name, and a directory lists, and a value reads and writes, the opener's namespace's; a name the reader's namespace has no interface of is found by a lookup and fails to open with `ENOENT`, where Linux fails the lookup. Values are per namespace and interface and writable by root; `disable_ipv6` acts, the rest describe packet handling and have no effect. The port range and `somaxconn` are read-only.
- **Unix sockets** stay a separate kernel facility. Their abstract namespace is per network namespace. Files passed with `SCM_RIGHTS` count against the sending user's `RLIMIT_NOFILE` until received (`CAP_SYS_RESOURCE` exempts), and sockets that only messages in flight keep alive are collected when a Unix socket is released, as Linux's `unix_gc` does.

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
  - `publish_datagram` returns a `BackendDatagram` bound to a host port, which serves whichever guest socket it is attached to.

  Backends wake, from their own threads, the queue of the guest socket an object serves, which is handed to them when the object is made: a socket has one wait queue for its whole life, so waiters that registered before the backend object existed are not lost. A published datagram endpoint exists before any socket, so attaching it switches the endpoint's queue, never the socket's.

**Where a connection goes**

| Destination | How it is carried | Port relationship |
| --- | --- | --- |
| Same namespace, including loopback | In-memory stream between the two sockets | One guest port space |
| Another machine on a virtual switch | Connection request and stream data routed to the machine that owns the destination address | Each machine has its own address; ports are per address |
| Anything else (host backend, the default) | The kernel opens a host socket on the guest's behalf | Guest port is virtual; the host picks its own |
| No backend | Fails as an unreachable network | Loopback only |

**Publishing** is the only place host and guest ports meet.

- **Explicit publishing:** the embedder maps a host port to a guest address and port. Each connection the host accepts is handed to the guest listener.
- **UDP publishing:** a host UDP socket bound to the published port serves the first guest socket bound where the publication reaches. Datagrams arriving there go to that socket, and its datagrams to outside addresses leave from the published port, so resolvers and NAT'd clients see replies from the address they asked. Other sockets sharing the port (`SO_REUSEADDR`, `SO_REUSEPORT`) hear from outside only once the first goes. With no socket attached, arriving datagrams are dropped, as Linux drops a datagram for a port nobody holds.
- **Auto-publishing (opt-in):** the kernel watches `listen`, and explicit UDP `bind`s, and mirrors the port on the host. Sockets bound only to loopback stay private.
- **Failure is the host's problem.** A host port that is already taken is a logged publishing failure, never an error returned to the guest.

**Policies:**

- **Gateway alias.** Whether one guest address reaches the host's loopback.
- **Peer address.** The host backend knows each connection's real peer, so `getpeername` reports it.
- **DNS.** `resolv.conf` points at resolvers the host backend can reach.

**Not supported:**

- raw sockets, and frames: a packet socket (`AF_PACKET`, with `CAP_NET_RAW`) exists, binds to an interface and answers interface requests, but no frame ever reaches it, and a send that passes Linux's checks (`ENXIO`, `ENETDOWN`) fails with `EOPNOTSUPP`
- ICMP datagram sockets (`SOCK_DGRAM` with `IPPROTO_ICMP`), so `ping` does not work
- netfilter
- MTU and fragmentation behaviour
- exact `TCP_INFO` values, which are synthesized

A raw socket fails with `EPERM` without `CAP_NET_RAW` and `ESOCKTNOSUPPORT` with it, an ICMP datagram socket with `EPROTONOSUPPORT`. Unprivileged ICMP sockets through a host ICMP socket, as Linux's `ping_group_range` allows them, or a packet-level stack as an optional backend can come later if something needs these.

### Port tables

`wasixv2-vnet`'s `Net` is one namespace. It keeps one port table per protocol, shared by IPv4 and IPv6 as Linux's bind hash is, so a dual-stack `[::]` socket collides with IPv4 sockets on its port unless it is `IPV6_V6ONLY`. IPv4-mapped addresses are stored as IPv4.

- **Explicit bind** (`inet_bind_conflict`): two sockets conflict when their addresses overlap (equal, or either a wildcard that covers the other), unless both set `SO_REUSEADDR` and the existing one is not listening, or both set `SO_REUSEPORT` with the same owner (the creator's filesystem uid, which accepted sockets inherit). `listen` re-checks, so of two `SO_REUSEADDR` sockets on one port only one can listen. UDP shares with `SO_REUSEADDR` on both, whatever the state.
- **Errors, in Linux's order:** `EADDRNOTAVAIL` for an address on no up interface (unless `IP_FREEBIND`), `EACCES` below `ip_unprivileged_port_start` (1024) without the capability the kernel passes in, `EINVAL` when already bound, `EADDRINUSE` on a conflict.
- **Ephemeral ports** come from `ip_local_port_range` (32768–60999) from a seeded random offset. `bind` to port 0 tries odd ports first (SO_REUSEADDR sockets the lower half of the range before the upper one, others the whole range) and fails with `EADDRINUSE`; `connect` tries even ports first, may share a port with other connected sockets that have a different peer, and fails with `EADDRNOTAVAIL`.
- Accepted sockets share the listener's port. Closing a socket frees its port at once: there is no `TIME_WAIT`.

### Streams and datagrams

- **TCP inside a namespace.** `connect` to a local listener creates the accepted socket at once and pairs the two with an in-memory connection. When the accept queue holds more than `backlog` connections, the client waits in `SYN_SENT` (`EINPROGRESS`) until `accept` makes room. Closing the listener resets queued connections and refuses waiting clients. A connection to a port nobody listens on fails with `ECONNREFUSED` right away.
- **Buffering.** Each direction queues up to the receiver's `SO_RCVBUF` plus the sender's `SO_SNDBUF`, counted in payload bytes. `SO_SNDBUF` and `SO_RCVBUF` are doubled and clamped as Linux does. Writes are partial when the queue fills, then `EAGAIN`.
- **Close and shutdown.** These follow `tcp_close` and `tcp_reset`. Closing with unread data or `SO_LINGER` zero resets the peer: its next call fails with `ECONNRESET`, or `EPIPE` once it has seen our FIN, and later calls see EOF or `EPIPE`. A write to a peer that closed normally succeeds once and then fails with `EPIPE`.
- **Readiness** is `tcp_poll`'s: a fresh socket is `OUT|WRNORM|HUP`, `RDHUP` follows the peer's FIN, `HUP` needs both directions shut, and `ERR` shows while `SO_ERROR` is set.
- **UDP.** Datagrams are copied between receive queues. A queue drops new datagrams once it holds more than `SO_RCVBUF`, charging each payload plus a fixed 768-byte overhead. Connected sockets only receive from their peer. A connected sender learns `ECONNREFUSED` when nobody is bound to its peer's port. `MSG_PEEK` and `MSG_TRUNC` work. Broadcast is accepted but not delivered. Sends fail in `udp_sendmsg`'s order (autobind first) and control messages are checked as `ip_cmsg_send` and `ip6_datagram_send_ctl` check them; `IP_PKTINFO`/`IPV6_PKTINFO` choose the source address and `UDP_SEGMENT` splits a send into datagrams, the rest describe packets and have no effect.

### Backends and the host bridge

The kernel-facing traits are in `vnet/src/api.rs`. Traffic for a destination that is not local goes through a route on the uplink interface (by default `eth0`, 10.0.2.15/24 via gateway 10.0.2.2), so the guest's source address is the uplink address and a virtual ephemeral port. Without a route the error is `ENETUNREACH`.

- The **gateway alias** (by default the gateway) maps to the host's loopback in both directions.
- **Publishing** (`Net::publish`, or auto-publishing of non-loopback listeners) hands `Backend::publish` a sink that delivers each accepted host connection to whichever guest listener holds the address at that moment. `getpeername` reports the real host peer. The backend accepts on the host only while that listener's accept queue has room: a full queue leaves connections in the host's backlog until `accept` makes room, as Linux leaves a SYN to a full queue unanswered, and a connection with no guest listener at all is reset. An auto-published host listener takes the guest's `listen` backlog, an explicit one `somaxconn`'s default.
- **Options**: what a host socket acts on (`TCP_NODELAY`, keepalive and its timers, `SO_LINGER`) follows the guest socket, including options set before `connect` or inherited from the listener.
- **DNS**: `NetConfig::nameservers` makes up `resolv.conf`. `vnet-host::host_nameservers` derives them from the host's file and routes loopback resolvers through the alias.

`wasixv2-vnet-host` runs one mio reactor thread per `HostBackend`, never tokio, and is Unix-only for now.

- Guest-side stream and datagram objects are buffers: 256 KiB per direction for streams, 256 KiB of queued datagrams. Guests read and write them without blocking. Outgoing datagrams go straight to the host socket: one the host cannot take is `EAGAIN`, the socket stops polling `OUT`, and the reactor wakes it once the host socket is writable again.
- The reactor moves bytes between the buffers and the host sockets. Guest activity marks a stream dirty and wakes the reactor through a `mio::Waker`. The reactor wakes guest wait queues from its own thread.
- Host errors keep their Linux errno numbers. Dropping the guest's handle flushes and closes the host socket, or resets it when unread data or `SO_LINGER` zero call for that. An orphan still flushing is reset once its peer has taken nothing for 60 seconds (standing in for `tcp_fin_timeout` and the orphan retries) or its `SO_LINGER` time is up.

## Execution

The execution crate is the only layer below the facade that knows about the wasm engine. It turns kernel tasks into running wasm, and turns guest control flow into kernel operations.

- **Loading.**
  - Wasm modules and `#!` scripts; the formats beyond `#!` are the embedder's, as Linux's binary format handlers are. There is no runtime registry like binfmt\_misc.
  - The kernel resolves the path and permissions; the loader only interprets bytes.
  - `#!` follows `fs/binfmt_script.c`: the first 256 bytes NUL-padded, a possibly truncated interpreter path is `ENOEXEC`, only spaces and tabs separate, at most one argument; argv becomes the interpreter, its argument, the name the script was run by (`bprm->interp`), then the old argv after argv[0]. As `exec_binprm`, five rewrites are allowed and a sixth is `ELOOP`; a script named through a close-on-exec descriptor is `ENOENT`.
  - `Kernel::exec` is `kernel_execve`: the facade's `Machine::run_file` starts a process by having it execute a guest path, so the first program gets everything `execve` does (scripts, set-id bits, `/proc/self/exe`), and its image then runs through the same loop as every image an `execve` loads. Only the program itself fails differently: one that does not compile or instantiate is the embedder's error, which `run_file` returns, as `run_module` does for a module, rather than the `SIGSEGV` of an image the guest's own `execve` loads.
  - What is not a script must be a format the embedder serves, decided before the point of no return as `search_binary_handler` does. The kernel reads the file's first 256 bytes (`bprm->buf`) and asks its `Binfmt`, which the facade's `Abis` is, whether they start an image: the loader's `probe` (by default wasm's magic and version 1). Only then does it read the whole file and ask again (`Binfmt::check`): whether some ABI serves the image's imports (`Abi::matches_image`, from the import section). Anything else, such as a module importing no ABI, a `wasix_64v1` module, a component or a module cut short, fails `execve` with `ENOEXEC` and the caller keeps its image; a module that passes both but does not compile still kills the process with `SIGSEGV`, as a corrupt ELF does past `begin_new_exec`.
  - The image goes to the execution layer as an `Image`: the file's bytes, shared rather than copied when its filesystem keeps them unchanging in memory (package volumes and commands; `Handle::shared_contents`). `Abis` carries a `Loader` that turns an image into a module and picks its engine; the default instruments and compiles. `wasmer run` plugs in one that picks the backend by the image's features, keeps the modules it compiled by the image's identity (the shared bytes' address) or the hash of a copy's bytes, so a shell loop compiles each program once, and goes through the compiled-module cache on a miss.
  - Past the point of no return, `de_thread` comes before anything of the new image is committed: the other threads are killed, and the caller waits, killable, until they are gone and the leader's instance has left the old image, or, for a leader waiting for its vfork child, the leader has left its memory (see Vfork below). Only then do the futexes of the old threads go to their waiters, the old memory end, and the credentials, descriptor table, signal dispositions and arguments change, so no old-image thread runs with the new image's. From a thread other than the leader, the thread then exits and the leader's instance runs the new image with the thread's signal state, as Linux's pid exchange does; a leader waiting for its vfork child gets the new image in an instance of its own, which the thread starts. A group that is exiting, or in another `execve`, fails it with `EAGAIN`, and `clone` fails with `EINTR` while an `execve` kills the threads.
- **Tasks and instances.**
  - A process image is one instance.
  - A thread created with a shared address space is another instance on the same shared memory.
  - Each task runs on a host thread or a coroutine, chosen by the host crate.
- **Suspension.** Syscall methods are async. A pending call suspends the guest with stack switching, which every wasmer backend we target supports. It works the same in every build configuration.
- **Build configurations.** All three work from day one:
  - **Exception handling with exnref** (the default). `setjmp`/`longjmp` use wasm exceptions.
  - **Exception handling plus PIC.** For programs that need dynamic linking.
  - **Asyncify.** For programs that need `fork`, which today means bash, and for guests that will be checkpointed. Binaryen's asyncify cannot be combined with wasm exceptions, so in this configuration `setjmp`/`longjmp` use the `stack-save` and `stack-restore` extensions, v1's `stack_checkpoint` and `stack_restore`, which only libc calls. Modules in other configurations get `ENOSYS` from them.
    - libc2 builds a second sysroot for it, `sysroot-wasixv2-asyncify`, without wasm exception handling; there `setjmp` is a function on `stack-save`, and `longjmp` one on `stack-restore`.
    - wasixcc selects it with `WASM_EXCEPTIONS=no`, as for wasixv1, and runs `wasm-opt --asyncify` with `asyncify-imports` naming the imports that unwind (`clone`, `clone3`, `stack-save`, `checkpoint`), so only code that can reach them is instrumented.
    - The execution layer recognizes an instrumented module by its `asyncify_*` exports and needs `__stack_pointer` exported, as libc-built modules do.
- **Return to guest.** After every syscall and at every preemption point, the hook runs pending work:
  - deliver signals: build the frame, run the handler, and have `rt_sigreturn` restore state
  - restart interrupted calls
  - swap the image after `execve`
  - wait while the thread group is stopped
  - tear down on exit
- **Signal frames.** The interrupted wasm call stack stays intact under the handler, so the frame is small.
  - **`mcontext_t`** holds only guest state a handler can clobber, such as the shadow stack pointer and the TLS base.
  - **`sigaltstack`** moves only the shadow stack. Whether the guest runs on its alternate stack is read from the shadow stack pointer, as Linux's `on_sig_stack` reads the user stack pointer, so a handler that switches to another context and never returns leaves nothing behind.
  - **The frame goes below LLVM's 128-byte red zone** under the shadow stack pointer, where a function that calls nothing keeps its locals without moving the pointer, as x86-64's `get_sigframe` does. Delivery at a preemption point can land in such a function.
  - **The trampoline calls the handler through its actual type.** The host writes the handler's parameter count into the frame, and libc's trampoline passes that many of `signo`, `info` and `uctx`. A native handler ignores the arguments it does not declare, so a mismatch with `SA_SIGINFO` is not fatal.
  - **`rt_sigreturn`** is the trampoline's return: the host restores the mask, the shadow stack pointer and the TLS base from the frame.
  - **Restarting is visible in the frame.** A restart slot says whether the interrupted syscall runs again after the handler. A handler can clear it, and the call then fails with `EINTR`; this is the wasm form of moving the saved program counter off the syscall instruction.
  - **Thread cancellation keeps musl's protocol.** musl's `SIGCANCEL` handler, installed with `SA_RESTART`, acts only when the interrupted program counter lies inside a cancellation point. On wasm, libc marks the cancellation point in the thread descriptor before it checks for a pending cancel, and the trampoline records the interrupted context's mark in the frame. The handler then clears the restart slot, and the cancellation point acts on the `EINTR`. Every other call restarts.
  - **`siglongjmp` out of a handler works across host frames.** The embedder's `Function::call` returns an error carrying the wasm exception, and the import returns it, which re-raises it through the remaining wasm frames. The host-side signal delivery code must pass these errors through untouched.
- **Coroutines.** wasixv1's context API (`context-create`, `context-switch`, `context-destroy`) carries over as `wasixv2` extension functions, and greenlet keeps using it directly. libc implements `getcontext`, `setcontext`, `makecontext` and `swapcontext` on top of it. On each switch, libc also swaps the shadow stack pointer and signal mask, passes arguments through a trampoline, and follows `uc_link`.
- **Fork.** Fork needs the asyncify configuration and goes through clone without CLONE\_VM. Asyncify unwinds the stack into linear memory, memory is copied, and both are rewound, so clone returns the child's pid in the parent and 0 in the child.
  1. The kernel asks the caller's address space whether it can be duplicated (`UserMemory::can_duplicate`), and fails with `ENOSYS` when it cannot, as Linux has no fork without an MMU. Only an asyncify instance whose import runs in the entry point's level says yes; see [Asyncify](#asyncify).
  2. `copy_process` copies the descriptor table, filesystem context, signal handlers, credentials and the address space's mappings and break (`dup_mm`), and leaves `GuestAction::Fork` for the caller.
  3. clone's writes that Linux makes after `dup_mm` wait for the copy, each in its own address space: `CLONE_PARENT_SETTID` in the parent's, `CLONE_CHILD_SETTID` and `CLONE_SETTLS` in the child's.
  4. The execution layer unwinds the stack, copies linear memory and the exported mutable globals (`__stack_pointer`, `__tls_base`) into a new instance on a host thread of its own, and rewinds both.

  On Linux hosts the copy of linear memory holds only the pages the parent touched, read from `/proc/self/pagemap` (probed once; without it the whole memory is copied, as on other hosts), less those that read zero, such as pages the guest only read; the child clears the pages its own instantiation filled outside them. A child thus commits what its parent wrote, not its whole memory and 8 MiB stack. The parent's other threads run during the copy, as they did during a full copy: each page of the child is the parent's page as it was at some instant of the fork, possibly torn by a concurrent write, and a write made before the fork is never missed, since its page fault set the page table entry the pagemap reads. On Linux no thread runs during `copy_page_range`.

  On those hosts a process's memory is also a private view of a frozen image, so parent and children share its initial pages copy-on-write: once instantiation filled the memory and before any guest code runs, the pages it touched (data segments) go into a memfd sealed against every change, and the memory is mapped over it with `MAP_PRIVATE | MAP_FIXED` at the same address, before the low guard is protected (`exec/src/base.rs`). The memory is then exactly the image plus the pages the process wrote since. The copy for a fork child holds only those, the pagemap telling them from the image's (file) pages, and the child maps the same image over its memory before writing them, which drops the pages of its own instantiation; the image is one per process image, shared by the whole fork tree until `execve`, and none for a memory that held nothing. The invariant holds everywhere the image backs: a range given back after a shared object is mapped from the image again with its data overwritten by zeros, never left anonymous, and no host `MADV_DONTNEED` ever drops pages there (it would show the image again). A child still instantiates the module, filling its data segments once before the image replaces them: that time is not saved, the memory is.

  Pages `MADV_WIPEONFORK` marked read zero in the child, as `dup_mmap` copies no pages of a `VM_WIPEONFORK` area: the kernel keeps the flag per page of private anonymous memory (a file or shared mapping refuses it with `EINVAL`; `MADV_KEEPONFORK` clears it), moves it with `mremap` as Linux's areas carry their flags, drops it with `munmap`, copies it into a fork child and starts an `execve` without it, and hands the ranges to the execution layer with the shared mappings (`SharedMappings::wipe_on_fork`), which leaves them out of a sparse copy and zeroes them in a full one, and where the image holds data under them, in the child after it mapped the image.
  5. The child has only the thread that forked, which may be any thread of the parent: it rewinds into that thread's entry point (`_start` or `wasixv2_thread_start`).

  Not supported: a new stack (`newsp`) and `CLONE_VFORK` without `CLONE_VM` are `EINVAL`, and fork from a signal handler is `ENOSYS`, since host frames separate the handler from the stack below it.
- **Vfork.** This is salvaged from wasixv1's setjmp-based vfork and works in all three configurations:
  1. libc's `vfork` is a macro that calls `setjmp` in the caller's frame, then calls clone with CLONE\_VM | CLONE\_VFORK, which returns 0.
  2. The kernel creates the child with a shared address space and blocks the parent task, as `CLONE_VFORK` does on Linux.
  3. The exec layer switches the instance's current task to the child, so later syscalls run as the child.
  4. When the child calls `execve`, the new image starts in its own instance. When it calls `exit` or a signal kills it, it becomes a zombie.
  5. Either way, the instance switches back to the parent task, and the host calls libc's exported `wasixv2_vfork_return(child)` from within the child's last call. It `longjmp`s to the vfork point, which returns the child's pid; the exception unwinds through the call as a `siglongjmp` out of a handler does.

  The parent's wait is killable, as Linux's `wait_for_vfork_done` is. A signal that would kill the parent wakes the child, and on the child's next return to the guest the parent's thread group dies there and then; so does a group that another thread's `exit_group` ends. The parent leaves its memory there and then, as Linux's runs `exit_mm` in its own `do_exit`: the kernel hands the instance a `GuestAction::ExitMm` for it, and the instance runs the parent's `exit_mm` on the memory it shares with the child, so the parent's robust futexes go to their waiters and its `clear_child_tid` is written back, as the child still uses the memory. Then the parent becomes a zombie, its parent is told, and the child goes to the reaper, while the instance goes on running the child. When the parent's image ended this way, the parent never returns from vfork: the kernel says so when it hands the instance back, and the instance runs as the parent without calling `wasixv2_vfork_return`, so the parent's exit unwinds it. Another thread's `execve` does not wait for the child either, as Linux's `de_thread` kills the leader in `wait_for_vfork_done`: once the instance left the leader's memory for it, the `execve` commits, and the thread that called it starts the new image as the leader in an instance of its own (`GuestAction::StartImage`) before it exits. The old instance runs the child on and ends with it, without going back to the leader (`Resume::Gone`); the run it belonged to, such as the embedder's, returns the status of the run that took over, which the image's instances share a handoff for. (A child that ends before the `execve` commits hands the instance back to the leader, which takes the new image there.) A thread other than the leader that waits in vfork is killed by any `execve` of its group in the same way (`zap_other_threads`, whose SIGKILL ends only that thread): once its instance left its memory for it, it counts as gone, the `execve` commits and the new image starts, and the instance goes back to the thread only to unwind it when the child ends. A child blocked in a wait that signals cannot interrupt, or stopped by job control, delays the parent's death until it returns to the guest.

  wasixv1 is brittle because it swaps its whole environment object in place and threads vfork state through exit, exec and process startup. Here a task switch is one pointer in the exec layer, and the kernel tracks the vfork relationship. The child shares the parent's address space, so its mappings and futexes are the parent's. libc keeps the stack of vfork points per thread, so threads vfork independently. As on Linux, returning from the function that called `vfork` is undefined. A call that bypasses the macro, such as `(vfork)()`, a function pointer or FFI, reaches libc's `vfork` function, which is `fork` without the atfork handlers, as musl's generic `vfork` is and POSIX allows; without asyncify it fails with `ENOSYS`, and its deprecated declaration makes the compiler say so. The function exists for link tests: without it autoconf's `AC_FUNC_FORK` writes `#define vfork fork`, and the program loses the macro. `<unistd.h>` leaves a `vfork` the program defined itself alone.
- **Preemption.** The loader adds interruption checks to every module: each function entry and loop header counts down a budget in a global of the instance, and when it runs out the guest calls a hook import that runs the return-to-guest path and hands out a new budget. A busy guest thus takes signals, `SIGKILL` and its group's exit within a bounded number of steps, on every engine backend. The start function gets no checks. A kernel work flag per task, set whenever work is queued for it, lets the hook and every syscall return skip the kernel when there is nothing to do.
- **Atomic waits.** The same pass turns `memory.atomic.wait32`, `wait64` and `notify` on a shared 32-bit memory into calls of hook imports, which trap as the instructions do and otherwise wait and wake on the kernel's futexes. The engine's own instruction blocks the host thread, where neither a signal nor the group's exit reaches the guest. A wait is a `FUTEX_WAIT` (64-bit for `wait64`), shared where the word lies in a mapped shared object: a signal interrupts it and its handler runs on the return-to-guest path; then an untimed wait goes on whatever the handler's `SA_RESTART` (`ERESTARTNOINTR`), since the instruction cannot fail, and a timed wait goes on to its deadline unless a handler ran; one that a handler ended returns 0, as woken, which makes its caller look at the word again as `EINTR` would. The group's exit ends it.
- **Traps.** A trap is a hardware fault: the thread group dies of `SIGSEGV`, `SIGILL`, `SIGFPE` or `SIGBUS` with the default action, as when a handler cannot run, and the host is told why. A trap while the group is exiting anyway is part of its teardown and changes nothing.
- **Task lifetime.** The execution layer releases each task once its instance is done with it. A process becomes a zombie, closes its files and notifies its parent only once its last thread is gone, as a zombie leader waits on Linux.
- **Dynamic linking.** Side modules are loaded and linked by the host; see the next section.

### Asyncify

`wasixv2-exec`'s `asyncify` module is the machinery of the asyncify configuration, and knows no ABI: wasixv2's `clone`, `stack-save` and `stack-restore` use it (exec's `fork` module), and a compatibility layer for another ABI can do the same.

- **Levels.** Every call the host makes into an asyncify instance goes through `asyncify::call` and is one level: the entry point, and the signal handlers and vfork returns the host runs from inside imports on top of it. An unwind reaches only the innermost level's call, because host frames sit between levels.
- **Unwind and rewind.** An import asks for the stack with `asyncify::unwind(request)` and returns. The guest unwinds to the level's call, which carries out the request and rewinds by calling the same function again. The rewinding guest calls the import again, and the import takes the outcome with `asyncify::resumed`, which ends the rewind, and returns what it says.
- **Requests.**
  - `Save { slot }` records the level's stack, saved from a guest address the import picks, such as its `jmp_buf`. The import resumes with the saved stack's id, which holds a generation unique to the instance; the import writes it into the `jmp_buf`, so a copy of those bytes restores the same stack wherever the guest keeps it, as a copied `jmp_buf` does on Linux (bash copies one aside, sets it again in a nested call and copies it back).
  - A saved stack stays restorable until the host sees that its frame returned: its level returned; a later save of the level was made with the shadow stack pointer above it, so a frame holding its save point was popped; or a later save from the same slot came from a stack of the same shape (stack pointer and unwound size), which is the same `setjmp` run again or a sibling call, since a nested frame always adds to the unwound stack. A copy kept aside while one frame sets the same `jmp_buf` again at the same depth is the one case this cannot tell apart, and is not supported.
  - `asyncify::restore(id, val)` gives the error the import returns: it discards the guest stack up to the level that saved `id`, through host frames, as an exception does, and that level rewinds the saved stack, whose import resumes with `val`. Memory stays as it is, and the frames from the entry point to the save point come back with their wasm locals as saved, which is `longjmp`'s contract. A stack can be restored only while its level runs.
  - `Fork { child, tag }` copies the instance for the task `child` and resumes on both sides with `tag`, which carries the ABI's own data (wasixv2's tid writes). Only the outermost level can fork; `asyncify::whole_stack` says whether an import runs there.
- **Memory.** Each instance takes a 1 MiB unwind buffer from the top of linear memory on its first unwind. Pages grown there count as mapped for the kernel, as pages the guest grows itself do. A deeper stack than the buffer holds traps.
- **Preemption.** The loader adds its checks after asyncify instrumented the module, so they also run while a stack unwinds or rewinds; the hook does nothing then, since a handler would find the instance mid-rewind.
- **The import that unwinds** must not run its effect twice, since the rewinding guest calls it again: wasixv2's `clone` is wrapped so the kernel sees it once.

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

- `RTLD_GLOBAL`, `RTLD_LOCAL`, `RTLD_NOLOAD`, `RTLD_NODELETE`, `RTLD_NOW` and `RTLD_LAZY` are flags with musl's values and meaning. The host keeps the global scope: the main module and `RTLD_GLOBAL` modules, in load order.
- Symbols are always bound at load time, as in musl. True lazy binding through a PLT has no wasm equivalent, and musl does not do it either. Its `RTLD_LAZY` only defers references to missing symbols, and that maps onto wasm: a missing function import gets a stub that resolves the symbol when called, and a missing `GOT.func` or `GOT.mem` entry, a mutable global, is filled in when a later `RTLD_GLOBAL` load defines the symbol. A call to a symbol that never appears kills the process. With `RTLD_NOW`, a missing symbol fails the load. To avoid a host call on every call through a stub, the host can generate the stubs as wasm that calls through a table slot.
- `RTLD_NODELETE` changes nothing: no module is ever unloaded, which is also musl's behaviour.
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
| `TokioHost` | `tokio` (facade default) | `SystemTime`, tokio `Instant` | tokio timers | OS (`getrandom`) | `spawn_local` needs a `LocalSet`; blocking work on tokio's blocking pool; guest threads on the starting thread's multi-thread runtime, each in a `LocalSet` (a current-thread runtime of their own otherwise) |
| `WebHost` | `web`, `wasm32-unknown-unknown` | `Date.now()`, `performance.now()` | `setTimeout`, re-armed until the deadline | `crypto.getRandomValues` | `wasm_bindgen_futures::spawn_local`; blocking work runs inline and stalls the event loop |

`Stdio::from_fns` wires stdout and stderr to callbacks with stdin at end of stream, which is enough for a browser console. A richer browser stdin is the embedder's own `ByteSource`. A terminal goes to `MachineBuilder::console` instead: its bytes pass through the kernel's line discipline, and the embedder passes on its size changes and hangup through the `Signaller`.

**Open items**

- **Guest threads.** `Executor` gets a method to start a task on a host thread, or on a web worker, when `clone` lands.
- **Exec in the browser.** `wasixv2-exec`, `wasixv2-dylink` and the facade build for `wasm32-unknown-unknown` with the `js` feature, since wasmer's js backend implements async host functions and `call_async` with JSPI (`WebAssembly.Suspending` and `promising`). Running a guest has not been tried yet. It needs a JSPI-capable engine, and guests built with exception handling need exnref support.
- **Shared memory.** libc-built guests import a shared memory, which needs `SharedArrayBuffer` and therefore a cross-origin isolated page (COOP and COEP headers).

## Function tables

`call-dynamic`, `reflect-signature` and the closure calls exist because a guest cannot call through a signature chosen at run time, inspect a table entry's type, or create a function. libffi needs all three. They keep wasix-libc's C shapes, and live in `wasixv2-dylink`, since they manage the function table and generated code as dynamic linking does.

- **`call-dynamic`** calls a table entry with argument and result buffers. Its strict mode requires the buffers to match the signature. Its lax mode follows native calling conventions as far as wasm allows, because C code routinely calls through a function pointer of the wrong arity (K&R `void (*)()` casts, CPython's `METH_NOARGS`, GLib callbacks): extra arguments are dropped, missing ones and missing results are zero. Types of the arguments both sides have must still match, as no native ABI defines those mismatches either.
- **Closures come from per-type stub pools.** wasixv1 compiles a module per closure, because it bakes the backing function and environment into the code. Instead, the host generates one module per function type exporting a batch of stubs, and grows the pool by another, larger batch when it runs out. Stub *k* reads its backing function and environment from entry *k* of an array in linear memory, in the host-owned arena dynamic linking uses for TLS, so a batch is compiled and instantiated once per type, and wasmer's module cache can reuse it across processes. `closure-prepare` takes a free stub of the right type, writes the two values to its entry, and places the stub in the closure's table slot.
- **One table write per closure remains.** libffi fixes a closure's code address in `ffi_closure_alloc`, before `ffi_prep_closure_loc` gives its type, and CPython's ctypes and cffi depend on that order. So `closure-allocate` reserves a slot without knowing the pool, and `closure-prepare` writes it in every thread, with the stop-the-world described for dynamic linking. Avoiding that would take a separate libffi port, which is not worth it.

## Recording, replay and checkpointing

These are two separate features. Recording is a syscall layer; checkpointing serializes kernel state directly.

| Feature | Where | How | Used for |
| --- | --- | --- | --- |
| Record and replay | Generic syscall layer | Logs each call, its result and every byte copied to the guest. Replay returns the logged results without running the kernel, as rr does. | Deterministic debugging, auditing, reproducing failures |
| Checkpoint and restore | Kernel crate plus a checkpoint crate | Serializes the kernel object graph and guest memory, then rebuilds them on restore, as CRIU does | Snapshots, fast start, migration; the stack being captured must be asyncify-instrumented |

Rebuilding state by re-running a syscall log would force the log to understand kernel semantics in order to compact it. Checkpointing avoids that.

**Guest-triggered checkpoints.** A guest can checkpoint itself with the `checkpoint` extension, v1's `proc_snapshot`. This is what lets a module built with exception handling, such as the PHP interpreter, work with instaboot. One carefully chosen call site plus a short list of asyncify-instrumented functions on the path to it are exactly the stack the host has to capture, and the rest of the module needs no asyncify. The call returns twice: 0 when the checkpoint is taken, and 1 in a process resumed from it, which must then reseed randomness and reconnect what it needs. A call path with a function that is not instrumented gets `EINVAL`, and an embedder without checkpoints gives `ENOSYS`.

Objects that point at the host, such as host files and published ports, are restored through their backends and may fail. Each such failure is reported per object, not as a whole-restore failure.

## wasixv1's extensions in v2

wasixv1 has about 95 syscalls of its own. Most become their Linux counterparts, and only those wasm cannot express from the guest remain extensions. libc2 keeps wasix-libc's C interface for them: the `<wasix/...>` headers (`context.h`, `call_dynamic.h`, `closure.h`, `reflection.h`, `function_pointer.h`, `value_type.h`) and v1's function names, so code ports between v1 and v2 unchanged.

| wasixv1 | v2 |
| --- | --- |
| `sock_*`, `proc_exec*`, `proc_fork*`, `proc_exit2`, `proc_id`, `proc_parent`, `proc_join`, `proc_signal`, `proc_signals_get`, `thread_exit`, `thread_id`, `thread_join`, `thread_signal`, `thread_sleep`, `thread_parallelism`, `futex_*`, `epoll_*`, `fd_dup2`, `fd_fdflags_*`, `fd_pipe`, `path_open2`, `chdir`, `getcwd`, `tty_get`, `tty_set`, `sched_yield` | Their Linux syscalls |
| `proc_spawn*` | musl's `posix_spawn`, through `vfork` and `execve` |
| `thread_spawn`, `callback_signal`, `context_*`, `dlopen`, `dlsym`, `dl_invalid_handle` | `thread-spawn`, `signal-callback-register`, `context-*`, `dl-*` |
| `call_dynamic`, `reflect_signature`, `closure_*` | The same, in [Function tables](#function-tables) |
| `proc_snapshot` | `checkpoint`; libc2's `wasix_proc_snapshot()` stays as a legacy wrapper that always returns 0, documented as such and pointing to `wasix_proc_checkpoint()`, which returns 1 when resumed |
| `stack_checkpoint`, `stack_restore` | `stack-save`, `stack-restore`, for asyncify modules |
| `port_*` (addresses, routes, gateway, MAC, bridge, DHCP) | Dropped. Interfaces, addresses and routes are netlink; bridging and DHCP are the embedder's network configuration, as they are userspace on Linux |
| `resolve` | Dropped. libc's resolver uses `/etc/resolv.conf` over sockets, as on Linux, which has no DNS syscall for applications |

## WASIX v1 compatibility

Programs built for WASIX v1 (`wasi_snapshot_preview1`, `wasi_unstable`, `wasix_32v1` and wasi-threads' `wasi` module, as wasix-libc calls them) run unchanged on the v2 kernel.

- **Where it lives.** `wasixv2-wasix1` is a second guest ABI of the execution layer. Exec's `Abi` trait has five ABI-specific spots: the import table, the process entry, the thread entry, signal delivery, and how a vfork parent resumes. `Abis` is the ordered registry that process images, `execve`d images and vfork children select through, so a v1 program may `execve` a v2 one and vice versa. The `wasixv2` ABI is its first entry; the facade adds `Wasix1` with its default-on `wasix1` feature.
- **The kernel stays v1-free.** Every v1 import is a shim that calls `Syscalls` and runs exec's ABI-neutral return-to-guest path (`complete`), exactly like the generated v2 shims. The trace, record and policy layers therefore see a v1 program as the v2 calls it makes. Anything v1 needs that Linux does not have lives in the shim layer: preopens, rights, v1 layouts and errno numbers, `args_get` over the startup block.
- **Layouts.** v1 structs are hand-written `GuestLayout` impls with explicit offsets, checked against `wasmer-wasix-types` (a dev-dependency only) and wasix-libc's static assertions, which win where the two disagree.
- **Translation through scratch.** v2 calls take guest addresses. A shim that must hand the kernel a struct with a different layout builds it in a 16 MiB scratch window at the top of the 32-bit address space (`CompatMemory`), above anything the guest's memory can reach, and reads results back from there. Buffers and iovec arrays, which have the same layout, pass through, so bulk data is still copied once. A v1 guest's memory cannot grow into the window: the kernel's growth stops below it, and so does the guest's own `memory.grow`, which the loader turns into a call the execution layer checks against the ABI's `memory_limit`. A guest address in the window is a bad address (`EFAULT`), except where the shim's own translated values lie during its call.
- **Startup.** `_initialize` (if exported), then `_start`; returning from `_start` is `exit(0)`, and returning from `wasi_thread_start` ends the thread. Preopens are v1's: the root (`/`), the working directory (`.`) and the root again, opened on the process's first v1 call so they land on descriptors 3, 4 and 5 as under v1. As under v1, closing a preopen succeeds and leaves it open (wasix-libc keeps routing paths to it), until another file is renumbered onto it. wasix-libc finds its preopens by asking descriptors from 3 up and stops at the first that is no preopen, so a program started with descriptor 3 open (`3>file`, a socket a parent passes) sees no filesystem, as under v1. Moving inherited descriptors out of the way would break the programs they are passed to, so this stays a listed limit.
- **Signals.** wasix-libc keeps its own disposition table and runs handlers from one export, `__wasm_signal(sig)`; the host only decides that it runs and restores the mask afterwards. The first `callback_signal` installs a placeholder kernel handler for every signal libc may handle, except those ignored at startup (so `exec` keeps them ignored) and those whose default is to be ignored (v1 never delivered `SIGCHLD`, and a handler would turn every child's exit into `EINTR`). libc holds signals back by naming an export it does not have (`__wasm_signal_blocked`), which is the calling thread's mask: v1 libc never sets one itself. wasix-libc before v2026-07-30.1 holds them back in `pthread_create` and never ends the hold (its `__restore_sigs` is compiled out), which would leave the creating thread and the new one deaf to every signal; the spawn ends it first, as that `__restore_sigs` would have. Later libcs keep their mask themselves. No handler restarts calls, as under v1. libc's main thread calls itself `0x3fffffff`, which `thread_signal` maps to the main thread.
- **Threads and futexes.** A v1 thread is a Linux thread started at `wasi_thread_start(tid, start_arg)`; returning from it is `exit`. `futex_*` are private futexes; `futex_wait` succeeds unless the address is bad, since libc traps on any error.
- **vfork.** The kernel's vfork model is reused unchanged. After `proc_fork_env` (wasm-exceptions builds) a v1 parent resumes when the child's `proc_exec*` or `proc_exit2` returns to it, and libc then longjmps to the vfork point itself. In asyncify builds vfork is `proc_fork` without a memory copy: the stack is saved at the call and the parent rewinds it when the child ends. `proc_spawn*` is `posix_spawn` inside one import: vfork, the file actions and dispositions as the child, `execve`, and the parent resumes. A vfork child's compat state is a copy of its parent's, registered by its pid.
- **fork and setjmp.** In asyncify builds, `proc_fork` with a memory copy is the kernel's fork served by exec's asyncify machinery, with the compat state carried to the child as the fork payload; `stack_checkpoint` and `stack_restore` are exec's saved stacks, as v2's `stack-save` and `stack-restore`.
- **Terminal.** `tty_get`/`tty_set` map onto `TIOCGWINSZ`, `TCGETS` and `TCSETS` on the stdio descriptors; without a terminal the modes are compat state, with v1's defaults. "Line feeds" is compat state either way: libc derives it from a flag that says nothing about the terminal, and v1 never applied it.
- **`wasmer run`.** Every WASIX and WASI program runs on the machine: v2 modules natively, v1 and WASI preview1 modules through `Wasix1`; the legacy `lib/wasix` runtime no longer runs anything there (it still resolves and loads packages, and serves `wasmer journal`, napi, swift and the C API). A module file is compiled once, with its preemption points, by the CLI's loader and runs through `Machine::run_module`; a package's command and a host `#!` script (which must lie in a `--volume`) run through `Machine::run_file`. Every image goes through that loader, which starts with the engine `wasmer run` picked, so backend options and the compiled-module cache apply. A precompiled module runs as compiled, without preemption points, and so does one the rewrite cannot handle, with a warning (`is_preemptible` tells an embedder). A terminal on standard input, with `wasmer` in its foreground, becomes the machine's console: the terminal is raw while the guest runs, standard output and error that show on it are the console too, and `^C` is the guest's own; `kill`'s `SIGINT` and `SIGQUIT` go to the console's foreground group, `SIGHUP` hangs it up, `SIGWINCH` resizes it, and the terminal gets its settings back on every exit, a panic included, and while `wasmer` is stopped. `--no-tty` keeps plain streams, with the three-press `^C` of a guest that ignores it. Plain `--net` is accepted and changes nothing, since guests always reach the host network; guest listeners are reached through `--publish`. `--use`, `--map-command`, network rulesets, `--http-client`, the journal options, napi and the `wasm_c_api_v0` imports are refused.
- **Networking.** v1 sockets are Linux sockets in the guest's network namespace: `addr_port` becomes a `sockaddr` in scratch, data moves through the guest's iovecs, and sends never raise `SIGPIPE`, as under v1. `poll_oneoff` is one `ppoll` (a clock timeout of 0 never fires, which wasix-libc relies on). epoll keeps v1's 24-byte `epoll_data` in a per-process side table keyed by (set, descriptor). `CONNECT_TIMEOUT` and `ACCEPT_TIMEOUT`, which Linux lacks, are applied by the shims. `resolve`, which v2 dropped, is a stub resolver in the compat layer: numeric names, `/etc/hosts`, then DNS to the `/etc/resolv.conf` servers over the guest's own sockets, so policy and tracing see its traffic. The machine's default root has an `/etc/resolv.conf` naming the host's name servers the guest can reach: a resolver on the host's `127.0.0.1` through the gateway alias, and for a host whose resolver is a stub on another loopback address (systemd-resolved's `127.0.0.53`) the stub's upstream servers. Without any, the lookup asks the local host, as libc does, and fails at once since nothing listens there. `port_addr_list` and `port_mac` read the interface table through netlink.
- **Package commands.** v1 applied a command's `wasi` annotation whenever a program started `/bin/<name>` or `/usr/bin/<name>`: `exec-name` as argv[0], `main-args` after it, `env` for names not set. Packages rely on it (an `npm` command is the node atom with `main-args`), so `proc_exec*` and `proc_spawn*` apply the `CommandDefaults` the facade registers (`Machine::set_wasix1_command_defaults`) to each `execve` candidate that names a command's file, by any path to it, as v1's binary factory did; the facade finds the files when the first process starts. A command that is a `#!` script's interpreter gets none. A native `execve` gets Linux semantics. Files v1 programs create are `0777` before the umask: v1 has no mode bits or `chmod` and started any file, so a script a v1 program writes runs. There is no finer rule to follow: wasix-libc sends no mode (its `open` drops it, and `chmod`, `fchmod`, `fchmodat` and `umask` do nothing), so a v1 bash that writes a script and `chmod +x`es it asked for nothing the host sees. Files created in a mapped host directory get those bits on the host too.
- **Not served.** wasm64 (`wasix_64v1`): the kernel's address spaces are 32-bit. `wasix_http_client_v1`. `port_*` configuration, which v2 dropped, and `port_route_list` until netlink answers `RTM_GETROUTE`. `thread_join` (no libc uses it; Linux would need pidfds of threads), the old bus-style `proc_spawn`, `proc_parent` of another process (needs `/proc`), and `proc_raise_interval` for signals other than `SIGALRM` (needs POSIX timers). Dynamic linking, closures and reflection return `NOTSUP` until they forward to their v2 extensions. Calls not implemented yet return `NOSYS`. Rights are reported, never enforced, so `NOTCAPABLE` never comes back. Signals whose default is to be ignored (`SIGCHLD`, `SIGCONT`, `SIGURG`, `SIGWINCH`) never reach v1 handlers, as under v1, so v1 `less`, `vim` or a shell miss window size changes and child exits by signal.

## Open questions

- **An in-process DNS server**, for sandboxes without direct DNS access, that `/etc/resolv.conf` can point at.

## Where wasm limits compatibility

These limits are why full compatibility stays a goal rather than a state we reach. Everything below is a known gap and must be listed in the compatibility promise.

| Area | Limit | Behaviour we choose |
| --- | --- | --- |
| Shared file mappings | No MMU, so no page can be shared with the page cache or another mapping | `memfd_create` and `/dev/shm` files on Linux hosts: shared mappings are the file's own pages in every process (read-only where they may never be written); pages past the end of the file read zero instead of raising `SIGBUS`. Other files, and every file elsewhere: copies synchronised with the file at `msync`, `munmap`, exit and around the process's own I/O on the file; other processes see changes once both passed such a point |
| Shared memory between processes | Wasm modules cannot share part of their memory; the host can place shared pages in a memory only on Unix, with wasmer's sys backend | Coherent on Linux and macOS hosts: `MAP_SHARED` anonymous memory and SysV segments are host shared memory that every mapping shows, across fork too, and futexes in them work between processes. Elsewhere `MAP_SHARED` anonymous memory stays private to its process, even across fork; SysV segment attachments are copies synchronised at `shmat`, `shmdt`, `msync` and IPC syscalls; futexes in them work only within a process. Pages `mremap` grows a mapping by past its object read zeroes instead of raising `SIGBUS` |
| `MAP_FIXED` | Wasm memory only grows at the end | Works over pages `brk` and `mmap` handed out and past the end of memory; the image's own pages cannot be replaced (`EINVAL`) |
| `mprotect`, guard pages | Linear memory has no page protection | Impossible; protections are not enforced, and `PROT_EXEC` fails with `EACCES`. The exceptions are the host's read-only guard over the first 64 KiB of stack-first programs (see [Guard page at 0](#guard-page-at-0)) and shared mappings that may never be written, which the host maps read-only |
| Fork | No copy-on-write of the pages a process wrote; the wasm call stack is outside linear memory | Asyncify configuration only, not from signal handlers; memory is copied eagerly (on Linux hosts only the pages the parent wrote; the image instantiation filled is shared copy-on-write), but for mapped shared objects, which the child shares; other configurations get `ENOSYS` |
| Async signals | Guests only stop at interruption checks | Engine interruption checks; latency is bounded by check density |
| `ptrace` | No way to stop and inspect a guest at the instruction level | Exported, returns ENOSYS |
| Scheduling | The host, not the kernel, schedules guest threads | Policies, priorities, nice values and affinity are kept, checked and reported per thread, but advisory; no real-time guarantees |
| Executable memory, JIT | Wasm code cannot be generated at runtime inside the guest | `PROT_EXEC` mappings fail |
| Native binaries | Only wasm is executable | `execve` of an ELF file returns ENOEXEC unless a registered format handles it |

## Crate structure

The new crates live under `lib/wasixv2/`, use `wasixv2-` package names, and start unpublished. Only `wasixv2-exec`, `wasixv2-dylink` and `wasixv2` depend on the wasm engine.

| Crate | Directory | Responsibility | Depends on |
| --- | --- | --- | --- |
| `wasixv2` | `api` | Embedding facade: assembles kernel, drivers, network backends, layers and exec; boot configuration | all of the below |
| `wasixv2-abi` | `abi` | Guest-visible constants, errno values, flags and struct layouts for wasm32 | — |
| `wasixv2-codegen` | `codegen` | Build-time generator: syscall table to import shims, decoders, trait, request enum | — |
| `wasixv2-pages` | `pages` | Host shared memory objects (memfd, POSIX shared memory) and placing their pages inside a host mapping; `Provider` makes them for a memory filesystem | — |
| `wasixv2-syscalls` | `syscalls` | The `Syscalls` trait, request enum, `Layered` adapter, user-memory types, result and restart types, the return-to-guest interface | abi, pages |
| `wasixv2-trace` | `trace` | Layer: syscall tracing | syscalls |
| `wasixv2-record` | `record` | Layer: record and replay | syscalls |
| `wasixv2-policy` | `policy` | Layer: syscall filtering and policy | syscalls |
| `wasixv2-host` | `host` | What the kernel needs from the host: executor, clocks, timers, entropy, shared memory objects | pages |
| `wasixv2-wait` | `wait` | Readiness: poll masks, wait queues, poll tables, blocking waits | abi |
| `wasixv2-vfs` | `vfs` | Mount trees, path walk, name cache, permissions, locks, notification, filesystem driver trait | abi |
| `wasixv2-vfs-mem` | `vfs-mem` | Memory filesystem | vfs |
| `wasixv2-vfs-host` | `vfs-host` | Host directory passthrough | vfs |
| `wasixv2-vfs-overlay` | `vfs-overlay` | Overlay filesystem | vfs |
| `wasixv2-vfs-package` | `vfs-package` | Read-only packaged filesystem images | vfs |
| `wasixv2-vnet` | `vnet` | Socket-level network per namespace: port tables, interfaces and routes, in-memory streams and datagram queues, publishing, the backend trait | abi, wait |
| `wasixv2-vnet-host` | `vnet-host` | Backend that carries outbound connections over host sockets and accepts published ports | vnet |
| `wasixv2-vnet-switch` | `vnet-switch` | Socket-level virtual switch that routes connections between machines by address | vnet |
| `wasixv2-kernel` | `kernel` | Tasks, processes, credentials, signals, namespaces, IPC, memory, terminals, pseudo filesystems, socket layer, Unix sockets; implements `Syscalls` | abi, syscalls, host, pages, vfs, vnet, wait |
| `wasixv2-checkpoint` | `checkpoint` | Kernel state checkpoint and restore | kernel |
| `wasixv2-exec` | `exec` | Loading, instances, suspension, return-to-guest, fork, preemption, shared pages in linear memory | syscalls, pages, wasmer |
| `wasixv2-dylink` | `dylink` | Side-module loading and linking (wasm extension) | exec, wasmer |
| `wasixv2-wasix1` | `wasix1` | The WASIX v1 guest ABI as a second exec ABI, served through `Syscalls` | abi, syscalls, exec, wasmer |

**Dependency rules**

- The kernel never depends on the engine, on layers, or on concrete filesystem drivers and network backends.
- Layers depend only on `wasixv2-syscalls`, so they work over any implementation, including fakes in tests.
- `wasixv2-exec` drives any `Syscalls` implementation through the return-to-guest interface and never reaches into kernel internals.
- Package and registry handling stays outside these crates. It appears only as the package filesystem driver.
- The existing `lib/wasix`, `virtual-fs`, `virtual-net`, `virtual-io` and `journal` crates are not dependencies of anything here.

## Decisions

| Date | Decision | Why |
| --- | --- | --- |
| 2026-10-28 | The kernel's copies honour the address space's page map: reading or writing an unmapped or `PROT_NONE` page, or writing one without `PROT_WRITE`, fails with `EFAULT`; the guest's own accesses stay unchecked. The `Mm` publishes its restricted ranges in a `PageAccess` the execution layer checks every copy against; the kernel's own fills of mappings bypass it (`force_read`/`force_write`, `FOLL_FORCE`) | Syscalls then fail on bad pointers as on Linux (LTP's `tst_get_bad_addr` and guard pages after test buffers, `clone302`), at the cost of one atomic load per copy while nothing is restricted; it supersedes "other protections stay unenforced" below for copies, without making `mprotect` a host call |
| 2026-10-28 | Futexes in the low guard fault where Linux's would in unmapped memory: an operation that reads or changes the word, a shared key, the robust list walk; the kernel asks the memory for its guard (`UserMemory::low_guard`) for words below `mmap_min_addr` | The read-only guard lets the guest's loads through only for clang's null `va_list`; a futex word there is never one, and `futex(NULL, FUTEX_WAIT, 0)` slept forever where Linux fails with `EFAULT` (LTP's `futex_supported_by_kernel`) |
| 2026-10-28 | `MAP_SHARED` anonymous memory and SysV segments live in host shared memory objects (`wasixv2-pages`), which the execution layer maps over their ranges inside the memory's static reservation; futexes there are keyed by object and offset | Wasm cannot share part of a memory, but the host can replace pages of a reservation that never moves: Linux's semantics (no synchronisation points, futexes between processes, process-shared pthread objects, LTP's `tst_checkpoint`) with no engine change. Copies stay the fallback where the host or the memory cannot |
| 2026-10-28 | Shared mappings that may never be written (`SHM_RDONLY`, a file open only for reading or write-sealed) map the object read-only on the host, as the low guard; the execution layer refuses host writes there under a per-memory lock | Linux's `SIGSEGV` for a store and `EFAULT` for a syscall, without copies; the lock keeps a host copy from racing a range that loses write access, which would kill the host; other protections stay unenforced, since mapping them would make `mprotect` a host call on every range |
| 2026-10-28 | memfd and `/dev/shm` files keep their data in host shared memory objects (`Provider`, `MemFs::with_shared_pages`), whose length never shrinks; their writable shared mappings map the object, read-only ones stay copies | Coherent `shm_open`/`memfd` sharing with no synchronisation points; a length that only grows means no mapping ever reaches past the object (no host `SIGBUS`), and truncation still frees memory by punching holes; mapping a read-only view would let the guest write a file it may only read |
| 2026-10-28 | The execution layer learns a fork child's shared mappings through `SharedMappings`, a supertrait of `Control` that `Layered` forwards by hand | `GuestAction` is `Copy` and `Layered`'s `Control` is generated; the same pattern as `AtomicWait` |
| 2026-10-28 | Each process image's memory is a `MAP_PRIVATE` view of a sealed memfd holding what instantiation filled; fork children map the same memfd and get only the parent's written pages | Parent and children share the data segments copy-on-write, exactly: the file never changes, so no process sees another's writes, with no remap of a running parent and no race with its threads. A per-module image would save the fd per process image but needs instantiation without data segments (a wasmer change) |
| 2026-10-28 | Fork copies the parent's touched pages, found through the host's `/proc/self/pagemap`, and leaves out those that read zero; a full copy where the pagemap is missing or does not work | A child commits what its parent wrote instead of its whole memory; the zero page counts as present in the pagemap, so a page only read is told apart by its content; checked once by probing a written and an untouched page |
| 2026-10-28 | One line discipline, the kernel's; a host terminal is raw and its bytes go to a console device, `/dev/hvc0`, which a new process gets as getty would open it | Echo, editing, `^C` and `^Z` must follow the guest's termios and its foreground group, which the host's discipline cannot know; Linux's `/dev/console` has no job control, so a shell needs the terminal behind it |
| 2026-10-28 | The target is Linux at the source level: clang defines \_\_linux\_\_ for wasm32-wasixv2, the libc2 sysroot ships Linux's UAPI headers, and Rust's target is target\_os = "linux", target\_env = "musl", vendor wasixv2 | The ABI is Linux's syscall interface, so code written for Linux and musl builds as it is: the explicit ports the 2026-10-14 decision asked for (gnulib, readline, mio, LTP) were Linux's code paths again. \_\_wasixv2\_\_ and the vendor still tell the target apart where it differs |
| 2026-10-27 | Package commands are files of a read-only layer under the root, and `wasmer run` starts the entry by `execve`ing `/bin/<name>`; v1's per-command annotations apply only to v1 callers, in the compat layer | Scripts, `PATH` lookup, `/proc/self/exe` and every ABI then work without the kernel knowing packages; argument rewriting by name is a v1 package rule, not Linux's |
| 2026-10-27 | libc has a `vfork` function besides the macro, and it forks; its declaration is deprecated | Link tests (autoconf's `AC_FUNC_FORK`) need the symbol, or programs replace `vfork` with `fork`; a call that bypasses the macro then still works where fork does, instead of failing to link. Without asyncify it fails with `ENOSYS` at run time, as fork does, so the compiler warns about such calls |
| 2026-10-16 | `memfd_create` files live on a tmpfs the embedder supplies (`Kernel::with_shm`), mounted outside the tree; seals live on the VFS inode | The kernel cannot depend on a driver; every write and size change passes through the VFS, so one check there covers all of them |
| 2026-10-16 | Magic links are a driver answer, `Link::Jump(path)`, that the walk continues from; drivers learn the caller only through `Ctx::task` in `get_link` | `/proc/self/fd/N` must reopen the object, not a path string that may be gone; only links depend on who follows them |
| 2026-10-16 | Every open file has a path; pipes, sockets and anonymous files get inodes on internal mounts | `fstat`, `/proc/<pid>/fd` names, fd-based metadata calls and reopening a pipe all follow from Linux's `f_path`, with no special cases per object |
| 2026-10-16 | `/dev` is an ordinary memory filesystem with device nodes; the kernel finds drivers by device number at open | Linux's devtmpfs and `chrdev_open`: an embedder's own root or a guest's `mknod` gets the same devices |
| 2026-10-27 | Futex waiters sit on the chains of a fixed table of hash buckets, each with its own lock, and record their key and a direct handle on their task; operations that compare the futex word on the waker's side read it holding the bucket locks without blocking, and retry after an unlocked read when that fails | Linux's `futex_hash_bucket`, `futex_q` and `futex_get_value_locked`: a wait, a wake and a waiter leaving touch one bucket, no wait allocates per sleep, and `FUTEX_CMP_REQUEUE` compares atomically with the requeue |
| 2026-10-27 | `SysErr::RestartNoIntr` (`ERESTARTNOINTR`) reruns a call after any handler | PI futex waits must never fail with `EINTR`, and neither can wasm's untimed atomic wait |
| 2026-10-27 | Wasm's atomic wait and notify instructions become hook imports on the kernel's futexes, through `AtomicWait`, a supertrait of `Control` that `Layered` forwards by hand | A guest parked in the instruction (Rust std's futex on wasm32) otherwise blocks its host thread past signals and the group's exit; a supertrait keeps every `Control` bound and the generated forwarding unchanged |
| 2026-10-16 | Preemption checks are added by the loader (a budget counted down at function entries and loop headers, then a hook import), not by the engine | wasmer has no resumable interruption; this works on every backend and needs no unsafe cross-thread writes |
| 2026-10-16 | Signal frames skip LLVM's 128-byte red zone, carry a restart slot the handler may clear and the handler's parameter count, and leave slots 16 to 31 to the guest | Preemption delivery can interrupt a red-zone leaf; musl's cancellation needs to turn a restart into `EINTR`; C handlers are called with mismatched `SA_SIGINFO`, which native ABIs tolerate |
| 2026-10-16 | Function table indices 1 and 2 stay empty (`--table-base=3`) | They are `SIG_IGN` and `SIG_HOLD`; a function there would be indistinguishable from them |
| 2026-10-16 | Tasks on the same memory share one address space object, which keys their futexes | A vfork child runs on its parent's memory; Linux keys private futexes by `mm` |
| 2026-10-16 | Fork is clone without `CLONE_VM` in the asyncify configuration; the kernel asks the caller's address space whether it can be duplicated, and answers `ENOSYS` otherwise | The engine knows whether the stack can be captured, the kernel what fork means; an early `ENOSYS` creates nothing, as on Linux without an MMU |
| 2026-10-16 | The asyncify machinery in `wasixv2-exec` is ABI-neutral (levels, unwind requests, saved stacks, fork images); wasixv2's imports sit on top of it | A compatibility layer for wasixv1's asyncify modules needs the same unwinding |
| 2026-10-16 | `longjmp` in the asyncify configuration discards the stack with an error that crosses host frames, then rewinds the saved stack at the level that saved it | One path for `longjmp`, `siglongjmp` out of a handler and the vfork return, as with wasm exceptions |
| 2026-10-16 | wasixcc's `WASM_EXCEPTIONS=no` selects the asyncify configuration and its own sysroot, `sysroot-wasixv2-asyncify` | Matches wasixv1's setting; asyncify cannot instrument wasm exceptions |
| 2026-10-16 | File mappings and SysV segment attachments are copies synchronised byte by byte with their object at defined points, against a shadow of the last synchronisation | Gives Linux's results for one process's own I/O and for data handed over under semaphores and messages, without an MMU; supersedes rejecting shared file mappings |
| 2026-10-16 | WASIX v1 is a second guest ABI of the execution layer (`Abi` registry), served by shims over `Syscalls` in `wasixv2-wasix1`, with v1 structs translated through a scratch window above the guest's memory | The kernel stays v1-free and the layers cover v1 programs; exec had exactly five ABI-specific spots; the host has no guest allocator to borrow |
| 2026-10-16 | A vfork child's end resumes the parent through a guest export, `wasixv2_vfork_return`, which `longjmp`s to the vfork point | One path for `execve`, `exit` and death by a signal; libc's `_exit` and `execve` stay unchanged, and no call returns where Linux never returns |
| 2026-10-15 | The VFS interns inodes per (superblock, inode number) and locks directories itself; drivers guarantee inode numbers are identities | Per-inode state (directory lock, `S_DEAD`, `I_LINKABLE`, the single directory dentry, later FIFOs, sockets and inotify marks) needs one place, and VFS checks must hold until the driver applies the change |
| 2026-10-15 | The executor is reached through `Host::executor`; tokio and the browser are host implementations behind features | Keeps the kernel runtime-agnostic and lets one embedder share an executor between hosts |
| 2026-10-14 | Crates live in this monorepo under lib/wasixv2, named wasixv2-\*, with the facade named wasixv2 | Keeping napi in a separate repo has been painful; the prefix keeps v2 apart from wasixv1 crates |
| 2026-10-14 | The libc port is a separate effort; the first slice is tested with hand-crafted binaries that use no libc | Keeps the kernel work unblocked by libc |
| 2026-10-14 | Keep wasixv1's setjmp-based vfork, with a task switch in the exec layer instead of swapping environments | The guest-side mechanism matches vfork semantics; only the host side was brittle |
| 2026-10-14 | One import module, wasixv2, for everything; kebab-case WIT names; Linux 64 suffixes kept, plain names may use 64-bit types | Simplest naming; the ABI is inspired by riscv32, not copied from it |
| 2026-10-14 | Three build configurations from day one: exception handling with exnref (default), exception handling plus PIC (dynamic linking), asyncify (fork and checkpointing). Stack switching suspends syscalls in all three. | Keeps wasixv1's three real configurations and drops its duplicate legacy-exception variants; stack switching is independent of the exception and asyncify choice |
| 2026-10-14 | Generated code is committed; CI regenerates it and fails on any difference | Reviewable diffs, IDE navigation, no build-time cost |
| 2026-10-14 | Syscall trait methods take a concrete caller type | Simpler than making every method generic |
| 2026-10-14 | The syscall table is written in the component model's format; its code generator is in development | Typed, versionable source of truth; the first implementation slice waits on the generator |
| 2026-10-14 | uname reports machine wasm32 and system name wasixv2 | It names the system that runs the program. The source-level target is Linux (2026-10-28), but the kernel is not: with a `Linux` system name, programs would read the release as a Linux version |
| 2026-10-15 | uname release is the ABI version, 2.0.0; nodename is localhost and domainname `(none)` until UTS namespaces exist | A Linux version would mislead programs that probe Linux features |
| 2026-10-28 | uname release is 7.2.0, the Linux version of libc2's UAPI headers; the version is `#1 SMP wasixv2 2.0.0` (supersedes the release of 2026-10-15) | Programs are built with `__linux__` against those headers now, and compare the release with the versions the headers document: 2.0.0 sent them down old-kernel paths. Unimplemented interfaces are `ENOSYS`, as on a Linux built without them; the version keeps the ABI marker |
| 2026-10-15 | Processes run as uid and gid 1 until credentials exist | Root (0) bypasses permission checks and sends programs down privileged paths |
| 2026-10-15 | The host instantiates and links side modules; libc2 owns policy and runs constructors | Wasm cannot create code from the guest, and threads, startup and per-instance state need the host; everything else stays in libc as on Linux |
| 2026-10-15 | During a link the host only calls wasm-ld's synthesized functions (`__wasm_init_memory`, `__wasm_apply_data_relocs`, `__wasm_init_tls`), never user code or `malloc` | Calls into user code mid-link invite re-entry, deadlocks and unwinding through host frames |
| 2026-10-15 | Side-module data and TLS come from a host-owned arena of kernel pages | A thread stopped while holding the `malloc` lock cannot allocate its own TLS through `malloc` |
| 2026-10-15 | libc2 passes the library search list as data; the host walks it through the `openat` path | Policy stays in libc, the walk is the kernel's, and startup reuses it |
| 2026-10-15 | Stop-the-world for `dlopen` is kernel-internal pending work delivered like a signal, not a signal number | The guest cannot block, ignore or observe it; this is Linux's `task_work` |
| 2026-10-15 | Closure stubs come from per-type pools, compiled and instantiated once per batch; closures still cost one table write each | v1's module per closure is wasteful, and libffi's API fixes the code address before the type |
| 2026-10-15 | `call-dynamic` keeps a lax mode for arity mismatches, defined precisely | C code calls through mismatched function pointers, which native ABIs tolerate and wasm traps on |
| 2026-10-15 | Guest-triggered `checkpoint` returns twice, 0 and then 1 | A resumed process must know to reseed randomness and reconnect |
| 2026-10-15 | libc2 keeps wasix-libc's `<wasix/...>` headers and function names | Source compatibility between v1 and v2 |
| 2026-10-29 | Veth pairs join namespaces at the socket level: a route out a veth end hands a `connect` or datagram to the peer namespace's sockets when it holds the destination; no forwarding, an unreachable neighbour is `EHOSTUNREACH` at once | LTP's network tests and `ip netns` setups need traffic between namespaces, and the socket layer already pairs sockets in memory; a packet path would need a TCP/IP stack |
| 2026-10-15 | `port_*` and `resolve` are dropped | Linux has no such syscalls: network configuration is netlink or the embedder's, and DNS is userspace |
| 2026-10-15 | `lseek` and `mmap` take 64-bit offsets directly, keeping riscv32's numbers; riscv32 only has `llseek` and `mmap2` | A wasm import can pass an `i64`; the riscv32 forms only work around 32-bit registers |
| 2026-10-14 | No shared file mappings and no memory protection; mapping support is limited, as in wasixv1's libc | Physically impossible on wasm linear memory |
| 2026-10-14 | The target reports itself as unix but not Linux: target\_family = "unix" and target\_os = "wasixv2" in Rust; clang defines \_\_unix\_\_ but not \_\_linux\_\_ | Low-level code is ported explicitly, as the existing wasixv1 forks already are, and defining \_\_linux\_\_ would not have spared any of them. Code that turns out fully compatible adds a wasixv2 case to its Linux check. Superseded on 2026-10-28. |
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
