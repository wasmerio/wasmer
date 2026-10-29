# WASIX v2: next steps

Working plan for WASIX v2 (see `DESIGN.md` for the design). It lists the work in flight, where each
piece stopped, how to finish it, the order to land it in, and the open decisions. Everything it
needs is on the remotes and in [`plan/`](plan/); nothing depends on the machine it was written on.
Update it when a step lands.

## State

`feat/wasixv2` at `f98fc48fa5c` (merge train 5) contains the kernel, exec, VFS, networking,
fork/exec/vfork, signals, threads, memory mappings and IPC, memfd, POSIX timers, fix-exec (traps as
signals, preemption, task lifetime), the exec `Abi` registry, the WASIX v1 compat layer
(`lib/wasixv2/wasix1`), package commands in `/bin` with `#!` and the cached image loader, atomic
waits on the kernel futexes, and the userland fixes. `wasmer run` runs WASIX v2, WASIX v1 and WASI
modules on the v2 kernel. tests `main` = `6405893`, libc2 `main` = `c26931b5`.

Suite on `feat/wasixv2` `8e19e34e15e` (merge train 10; wasixv2-tests `eb945b2`, libc2 `1e68cf76`,
abi `340b9f3`, wasixcc `7ab1ffb`, userland `userland-3`): 3233 pass, 662 expected failures, 835 ignored,
and one unexpected failure in the suite's own `ptimer-abstime`, a test flaw fixed in `eb945b2` (0
unexpected on a filtered recheck).

Smoke tests pass: v2 static-web-server (28/28); through the v1 compat layer
wasmer/static-web-server, phpix/phpix-85-32bit (CLI and `-S` server), coreutils, cowsay,
quickjs; SIGTERM of v1 servers without the host panic; `wasmer/bash` with commands in `/bin`.
python/python needs dynamic linking (owned by another team, out of scope for this plan).

**Merge trains 5 (§1) and 6 (§2) have landed** (see integration.md): `feat/wasixv2` also has WP3
(futex + mm), WP5+6 (host network bridge, Unix sockets) and WP9 (signals), plus the vfork-parent
exit fix. tests `main` = `5af6416`, libc2 `main` = `a524136a`.

WP1 (exec/de_thread) has landed too: tests `main` = `eac8874`, libc2 `main` = `bb5d6d71`.

WP4 (fs/vfs, 18 findings) has landed too: tests `main` = `7fd6227`, libc2 `main` = `0bc9eed3`.
WP4 left open: L-FS12's single-reference position-lock skip and per-step attribute cache (VFS design
changes), and FIGETBSZ belongs in wasixv2-abi (defined locally in the kernel for now).

WP2 (rest) and WP7 have landed too: tests `main` = `d54b93c`, libc2 `main` = `f216f303`. A v2 bash
fork+exec loop is about 12x cheaper (49 ms to 4 ms per `/bin/true`). v1 DNS works on
systemd-resolved hosts (H11, no in-process server). The wasix1 0777 create mode stays: v1
wasix-libc never passes a mode and its `chmod`/`umask` are no-ops, so there is nothing to match
(documented, L-W4).

WP10 (facade/CLI) has landed too (`11d0ae2781b`, wasmer only). User-facing: programs get `HOME`
(passwd) and `PATH=/usr/local/bin:/bin:/usr/bin` by default; `wasmer run ./script` maps the script's
directory read-only at `/run/wasmer/script`; `--use` works; a first program that cannot start exits
1 with the load error (python/python again prints "instantiation failed"); three Ctrl-Cs exit even
when SIGINT is ignored. Embedders: `Machine::shutdown()`, `Drop` kills, the `Layer` API. Deferred:
real Ctrl-C (job control), webc's `Container::downcast` (never succeeds in webc 12.0.1), shared
`st_dev` for two mounts of one layer. `train5-extras.sh`'s bash line now prints `/bin/readlink`
(the default PATH searches `/bin` first).

WP8 (performance) has landed too (`9d04b0b7899`): v1 pid cache, guest copies without allocation, v1
host functions only for what a module imports, and `AsyncFunctionEnvMut::try_read`/`try_write` in
`lib/api` (js backend not compiled here). Measured against WP10: v1 read −10%, write −11%, getpid
−5%, v2 read −5%, v1 thread create+join 190 to 59 us, v1 spawn 498 to 318 us. Dropped as
unhelpful or wrong: `nbytes = 0` (v1 libc's FIONREAD reads it), one `rt_sigaction` pass, a reusable
scratch, an errno table. Benchmark kept under `logs/wp8/bench` on the orchestration machine; proposed
home `bench/syscalls/` in wasixv2-tests.

The ABI-codegen package has landed too (abi `main` `832bb78`, pinned by wasmer `dba44c5eba2` and
libc2 `main` `3bcd192c`): generated v2 imports are built only for what a module imports (v2 thread
create+join 358 to 91 us, v2 spawn 620 to 259 us), and `Control::take_image` returns an `Image`
(the `ExecImage` supertrait is gone). Host code only: the libc2 sysroots are identical.

Merge train 7 has landed too (`c4c59c64f70`): kernel follow-ups (fatal-signal futex/mm release,
`clear_child_tid`, robust-list order, the `sigprocmask` lost-signal fix behind `pthread_once/6-1`,
`TFD_TIMER_CANCEL_ON_SET`), init as pid 1 (`--no-init`), killable vfork phase A, WP11 (incl. the lock
sweep: poisoned locks recover), a read-only 64 KiB guard at address 0 (`mmap_min_addr`),
`/etc/passwd`/`group` with `nobody`, `__linux__` (DESIGN; libc2 ships Linux's UAPI headers) and the
runner changes (uid 0, asyncify for forking tests, sysroot UAPI headers, per-test memory scopes,
bounded output capture, `bench/syscalls`). LTP's wasix-org fork carries a heartbeat fix for init on
branch `wasixv2-train7` (`bee8e8197`); its `wasixv2` branch should be fast-forwarded by its owners.
`__linux__` needs a release of the wasix-org LLVM fork (branch `wasixv2-linux-macro`, `3dfdb0509`)
by its owners before downloaded toolchains have it; this machine uses a local build.

Merge train 8 has landed (`b75d9d49205`; wasixcc `feat/wasixv2` `d6373e5`): C++ for the v2 sysroots (libc2 `libcxx`, wasixcc `cxx-v2`; exceptions
in the EH sysroot, none in asyncify), kernel follow-ups 2 (faulting reads keep data, FICLONE*,
uname 7.2.0, splice/tee/vmsplice), job control phases 1-3 (stop/continue, console tty with raw mode
and `--no-tty`, ptys), LTP shell tests in the runner (`WASIXV2_TESTS_USERLAND`), kernel follow-ups 3
(other processes' CPU clocks, a killed vfork parent's exit_mm at kill time, vfork phase B, reviewed),
the sparse fork copy (pagemap), and shared memory across processes (shared anonymous mappings, SysV
segments, memfd and /dev/shm files on host shared pages; futex keys on shared objects; LTP branch
`wasixv2-shared`). Designs: `~/wasixv2-orch/notes/design-*.md` on the orchestration machine.

Merge train 9 has landed (`0bd23caafbb`): read-only shared views, a non-leader thread parked in vfork
no longer blocks a sibling's execve, futexes in the low guard fault, MADV_WIPEONFORK/KEEPONFORK, LTP
tests run as non-leaders (runner launcher) with LTP's IPC as upstream (LTP fork `wasixv2-ipc`),
inbound UDP and `--publish host:port/udp` and the socket error queue, namespace scaffolding
(NsProxy, nsfs, `/proc/<pid>/ns`, unshare/setns/clone3, `NS_GET_*`), real CPU time accounting
(rusage, times, CPU itimers, RLIMIT_CPU, CPU-clock timers; sampling at 1/256 syscalls and preemption
points), and wasixcc's clone3 asyncify fix. Flaky (with reasons): nice05, epoll_wait16, malloc-brk-fail
and setenv-oom (host memory; fixed on `wasixv2/vmfill-commit`), tgkill01 (LTP in-process heartbeat
SIGUSR1; being fixed), fs_inod01 (timeout under load).

Merge train 10 has landed (`8e19e34e15e`): UTS/IPC/net namespaces, mount(2)/umount2/pivot_root/
mount_setattr, mount namespaces with propagation, the new mount API and statmount/listmount,
copy-on-write fork (32 MiB data: 0.13 MiB host RSS per child), reused pages dropped instead of written
(t_vmfill 4.2 GB to ~45 MB), pidfd on /proc dirs, RLIMIT_MEMLOCK, socket-netns ioctls, /proc/net/unix,
console drain on exit, lo down in new netns (SIOCSIFFLAGS/RTM_NEWLINK), kernel copies honour page
protections (EFAULT on PROT_NONE/unmapped), CLONE_PIDFD, the tgkill/heartbeat fix (per-task signal
pending, complete_signal targeting), and the new userland (util-linux mount, tar/gzip/cpio/unzip,
procps, psmisc; host triplet linux-musl). LTP's fs_bind shell tests (99) now run.

Merge train 11 (next): pid namespaces (+ runner registers *_NS kconfig tests), signal-driven I/O
(F_SETOWN/O_ASYNC/SIGIO), libc2 fork-style clone(), follow-ups 9 (nr_open, net conf, CLONE_VFORK without
CLONE_VM; CLONE_VM processes in progress), follow-ups 10 (LTP guarded buffers, stack-aware mlock
accounting, msync EBUSY on locked pages, umount busy check, listmount fault), follow-ups 11 (TCP urgent
data/SIGURG, dnotify, leases; in progress).

Open follow-ups: memory accounting (local branch, paused by the user), fork copy-on-write (memory
design phase 2b), `mmap/24-1` peaks at 8 GB, mlockall ignores RLIMIT_MEMLOCK (mlockall03), no `ldd`
in the userland, the guard as PROT_NONE once the LLVM null-va_list issue is fixed (and LLVM fork
releases for `__linux__` and that fix, by its owners), Windows guard, the LTP fork's `wasixv2`
branch to be fast-forwarded by its owners, C++ in released sysroots needs the fork's clang in libc2
CI. Flaky: `aio_suspend/1-1`, `fcntl14`, `timer_getoverrun/2-2`.

Follow-ups from WP2/WP7:
- `timer_getoverrun/2-2` fails about half the time alone ("0 overruns sent; expected 75"), on
  every binary; usually passes under suite load. Flaky-list candidate.
- `pthread_once/6-1` hangs about 1 in 10 runs (before WP2/WP7 too): all threads `Exit`, the main
  thread never returns from `pthread_join` — probably a lost futex wake at thread exit. In
  flaky.txt until fixed.
- A first program that fails to instantiate now exits 139 with "killing it with SIGSEGV"
  (python/python) instead of a plain load error: `run_file` goes through the execve image loop
  (M-CLI2). Report a plain load error for the first image (WP10).
- v1 command defaults don't apply to a command used as a `#!` interpreter.

Follow-ups found while landing train 6 (not yet scheduled):
- A fatal signal kills a group without releasing robust/PI futexes or writing back shared
  mappings (Linux `do_exit`). Proposed: on a pending group exit the exec layer hands the kernel the
  task's memory (futex release + `mm_release`) before unwinding. Needs agreement.
- `execve`/`exit_group` take the siblings' robust lists before the siblings are killed; a lock
  taken in between is not released.
- `sys_exit` writes `clear_child_tid` for the last thread too (Linux: only if `mm_users > 1`).
- A SIGKILL to a waiting single-threaded vfork parent takes effect only when dequeued (M-P9's
  "not killable" gap).
- L-F6 `TFD_TIMER_CANCEL_ON_SET`. Proposed: record the realtime-to-monotonic offset when an
  absolute CLOCK_REALTIME timerfd is armed; on expiry/read a changed offset gives ECANCELED. Needs
  agreement.
- A non-leader `execve` while the leader waits in vfork waits for the vfork child (Linux doesn't);
  same root as M-P9's "not killable" vfork wait.
- WP3's L-F2, L-F8, L-F9 need no change (cited in the WP3 notes).

## Branch map

Nothing in flight is on `feat/wasixv2` or any `main` yet. All branches are on origin. Repositories:
wasmer = wasmerio/wasmer, tests = wasix-org/wasixv2-tests (submodule `tests/wasixv2` of wasmer and
`wasixv2/tests` of libc2), libc2 = wasix-org/libc2, abi = wasix-org/wasixv2-abi (submodule
`lib/wasixv2/codegen` of wasmer and `wasixv2/abi` of libc2), wasixcc = wasix-org/wasixcc.

| Work | Repo | Branch | Head | Base | Commits |
| --- | --- | --- | --- | --- | --- |
| landed | wasmer | `feat/wasixv2` | `bdcdb958bef` | | |
| | tests | `main` | `b0861b8` | | |
| | libc2 | `main` | `5d33ff2a` | | |
| | abi | `main` | `bcd7622` | | pinned by wasmer and every branch below |
| | wasixcc | `feat/wasixv2` | `8222197` | | |
| train 5 | wasmer | `wasixv2/train5` | `0269525c9eb` | `bdcdb958bef` | 19, last is a `wip:` tests bump to `6405893` |
| | tests | `train5` | `6405893` | `b0861b8` | 5 |
| | libc2 | `train5` | `0006659a` | `5d33ff2a` | 4, last is a `wip:` tests bump to `6405893` |
| WP3 futex + mm | wasmer | `wasixv2/review-futex-mm` | `52098cb3951` | `1228c8983cc` (4 behind `bdcdb958bef`) via `wasixv2/v1-sigterm` | 12 = the 2 v1-sigterm commits + 10 |
| | tests | `review-futex-mm` | `034c3fa` | `aae6f73` (before `b0861b8`) | 2 = `7795723` (also in train 5) + `034c3fa` |
| WP5 + WP6 net | wasmer | `wasixv2/review-net` | `70c542fa87b` | `bdcdb958bef` | 1 |
| | tests | `review-net` | `45b0716` | `b0861b8` | 1 |
| WP9 signals | wasmer | `wasixv2/review-signals` | `1f3950f419d` | `bdcdb958bef` | 2: fix `8d686147337` + `wip:` tests bump to `0a9779e` |
| | tests | `review-signals` | `0a9779e` | `b0861b8` | 1 (`wip:` subject) |
| plan | wasmer | `wasixv2/plan` | this commit | `bdcdb958bef` | this file and `plan/` |

Superseded, not pushed, ignore: wasmer `wasixv2/bin-commands-wip` and `userland/jmpbuf-copy-proto`
(local prototypes of bin-commands and userland-fixes). The branches train 5 merged
(`wasixv2/bin-commands` `33f4515728e`, `wasixv2/v1-sigterm` `cee6fd48603`,
`wasixv2/userland-fixes` `9a8c6df66b0`, libc2 `userland-fixes` `56931561`) are superseded by the
train 5 branches; don't merge them separately.

No in-flight branch changes the ABI; all pin abi `bcd7622`.

## Merge order and expected conflicts

1. Train 5 lands first, as is (section 1).
2. Then merge train 6 = WP3, WP5+6, WP9, in that order, onto the new `feat/wasixv2` (section 2).
   The review branches were all cut before train 5. Conflicts, checked with `git merge-tree`:
   - WP3 must be rebased with `--onto`, dropping the two v1-sigterm commits (`a96aa7894d8`,
     `cee6fd48603`), which train 5 carries as `e085f5a0542` and `32b3886aff3`. Done that way it
     applies cleanly. A plain merge or rebase instead conflicts in seven files (`exec/src/atomics.rs`
     add/add, `kernel/src/futex.rs`, `kernel/src/lib.rs`, `syscalls/src/control.rs`, …).
   - WP5+6 applies cleanly on train 5 + WP3.
   - WP9 on top of those conflicts in three places:
     - `kernel/src/signal.rs` `real_timer_expired`: WP3 re-arms ITIMER_REAL when SIGALRM is
       dequeued, and WP9 makes `send` return a `Result` (RLIMIT_SIGPENDING). Resolve to
       `let _ = group.send(None, kernel_info(Signal::SIGALRM.0)); None`.
     - `DESIGN.md` vfork: keep WP9's new paragraph ("The parent's wait is not killable …") and
       train 5's rewritten paragraph after it (the vfork function, deprecated, ENOSYS without
       asyncify), not the old "libc has no `vfork` function" text.
     - `tests/wasixv2`: drop WP9's `wip:` bump. Point the submodule once, at the train 6 tests
       commit.
   - A throwaway tree of train 5 + WP3 + WP5+6 + WP9 with exactly these resolutions passes
     `cargo fmt --all --check` and
     `cargo check -p wasixv2 -p wasixv2-kernel -p wasixv2-exec -p wasixv2-wasix1 -p wasmer-cli --features wasmer-cli/cranelift --tests`,
     and `cargo test -p wasixv2-kernel -p wasixv2-wait` passes (226 + 24). Nothing else was run
     on it: no clippy, no other crate tests, no suite.
   - Semantic overlaps to review even though they merge cleanly: process exit, where train 5's
     `exit_files`/`closed_by` (`926bb24cc9f`), WP3's futex release on `exit_group`/`execve`
     (`3680124ec33`) and WP9's vfork-deferred `exit_notify` and `PR_SET_PDEATHSIG` all touch
     `process_exited`/`thread.rs`; signal dequeue (WP3's itimer re-arm and WP9's sigqueue charge);
     `exec_commit` (train 5's M-P8/H6 and WP9's pdeathsig clearing). Follow the lock order in
     [the review §4](plan/v2-review.md#4-lock-ordering-model-as-inferred-for-the-wp-authors): `Signals.state`
     is dropped before timers fire, and futex buckets are never taken under `Signals.state`.
   - Tests: on tests `main` = `6405893`, `git rebase --onto 6405893 7795723 review-futex-mm`
     (`7795723` is train 5's `a14fb83`; `034c3fa` then applies cleanly), then cherry-pick `45b0716` and
     `0a9779e` (drop the `wip:` from its subject). No conflicts.
3. Then WP1, WP2 rest, WP4, WP7 (they touch code train 5 merges), then WP8, WP10, WP11 (section 3).

Rules for every step ([rules](plan/rules.md)): push fast-forward only, never force. A rebased
branch goes up under a new name (for example `wasixv2/review-net-2`). Only merge/bless agents edit
`expected-failures/`; fix agents report the tests that newly pass. Commit bodies end with the
agent attribution lines of rules 7.

## 1. Land merge train 5

Train 5 merges `wasixv2/bin-commands` (package commands as files in `/bin` and `/usr/bin`,
`#!` per `fs/binfmt_script.c`, `kernel_execve`, a pluggable `Loader` with the module cache),
`wasixv2/v1-sigterm` (`memory.atomic.wait*/notify` on the kernel futexes, so v1 servers exit on
SIGTERM) and `wasixv2/userland-fixes` (asyncify saves by the generation in a copied `jmp_buf`,
`/dev/fd/N` of a pipe without a partner wait, large device reads; libc2's `vfork` function). On top
come the review fixes H6 (`cb7ba2abef0`, ENOEXEC before the point of no return through a `Binfmt`
probe), M-P8 (`d511736653d`), WP12 (libc2 `a838d039`, wasmer `5b5d17fe57f`) and one more fix,
`926bb24cc9f`: an exited process's files close by pid, which fixes a host panic ("syscall from an
unknown task") when two tasks of a process are released at once on SIGTERM. The full merge log is
[integration.md](plan/integration.md) (train 5 section).

**Where it stopped.** Last commit `0269525c9eb` (the `wip:` bump). The final tree `926bb24cc9f`
passed: 521 crate tests, clippy with the lint flags, fmt, CLI `--test run` (51 passed, 13 ignored;
`issue_3794` fixed, 5 new tests), and a release build. Both suite runs and the smoke tests
(sws-v2 28/28, v1 8/8) were on binaries *before* `926bb24cc9f`.
Not verified:
- a suite run with `926bb24cc9f`;
- the SIGTERM extras showing no panic (before the fix, v1 sws and `php -S` stopped with exit 127
  and the host panic);
- an exec speed comparison.

**How to finish.**

1. Set up the toolchain (section 6) with libc2 at `train5` (`0006659a`) and its two sysroots built.
   Check out wasmer `wasixv2/train5` with its submodules.
2. `plan/gates.sh <wasmer> release`, then `plan/suite.sh <wasmer> <libc2>/build-wasixv2 <out> copy`.
   Expect 2349 pass and 0 unexpected, and `diff -r <out>/ef tests/wasixv2/expected-failures` empty
   apart from flaky entries. A difference means `926bb24cc9f` changed behaviour: investigate it, and
   don't bless it away.
3. Smoke: `SWS_WASM=<sws.wasm> plan/smoke/run-all.sh <wasmer>/target/release/wasmer` (sws-v2
   28/28, v1 8 pass + python expected-fail), and `plan/smoke/train5-extras.sh <wasmer>` with
   the userland built (section 6). Both SIGTERM checks must exit with no "unknown task" panic in
   their logs. `wasmer/bash` must print 85, `hi`, `/usr/bin/readlink` (`/bin/readlink` since WP10's default PATH).
4. Exec cost: build `feat/wasixv2` (`bdcdb958bef`) release as the base binary and time
   `cargo run --release --manifest-path tests/wasixv2/Cargo.toml -- exec-exit-with-threads --exact`
   (and `proc-` tests) with each binary. Train 5 caches compiled modules, so it should not be
   slower. Record the numbers in integration.md.
5. Decide on `f71e0b1a38b` (wasix1: files v1 programs create are 0777 before the umask; open
   decision below). To drop it, rebase train 5 without it, and rerun step 2's `--test run` and
   the userland bash smoke.
6. Land, fast-forward only, in this order:
   1. tests: `git push origin 6405893:main`.
   2. libc2: on `train5`, reword `0006659a` without `wip:` (the pointer is already `6405893`),
      then `git push origin HEAD:main`.
   3. wasmer: on `wasixv2/train5`, reword `0269525c9eb` without `wip:`, then
      `git push origin HEAD:feat/wasixv2`.
7. Rebuild every shared libc2 sysroot from libc2 `main`: `make wasixv2-sysroot`, then
   `make wasixv2-sysroot-asyncify`, one after the other; cmake must be on `PATH`.
8. Update this file's state section and integration.md.

## 2. Merge train 6: WP3, WP5+6, WP9

Each review branch below says where its agent stopped. Finish the branch's own steps on the branch
(rebased first, then pushed under a new name), then merge as in "Merge order".

### WP3 futex + mm (`wasixv2/review-futex-mm`)

The agent was killed and left no state file; this section reconstructs its state from its commits,
its tests branch and its logs.

**Where it stopped.** Last commit `52098cb3951`. Ten WP3 commits on top of the two v1-sigterm ones:

| Commit | Change | Findings |
| --- | --- | --- |
| `2e9849d3cb2` | `mremap` checks as `check_mremap_params`; overlap EINVAL; `MREMAP_DONTUNMAP`; `old_len == 0` duplicates shared mappings | H2, L-F10 |
| `2a5133d9f20` | wait: `Waiter::handle` (direct wake handle), `Waiter::watch` (interrupt registered once per wait) | M-F3 (allocations) |
| `c511cd75b6b` | exec: `SysErr::RestartNoIntr`, always rerun unless the handler clears the restart slot | M-F2 |
| `1a7684d3820` | futex: hashed buckets (256 per CPU) with per-bucket locks, key in the entry, drop guard, PI waiter states, `CMP_REQUEUE`/`WAKE_OP` compare under both bucket locks, PI waits return `RestartNoIntr`, untimed `memory.atomic.wait` uses it, `WAKE_OP` shift masked, DESIGN futex-key text | M-F1, M-F3, M-F4, M-SIG1, L-F3, L-F11 |
| `3680124ec33` | `exit_group`/`execve` release every task's robust list, PI futexes, `clear_child_tid` | M-F5 |
| `be61e7fdfdc` | checked guest pointer arithmetic (`msgp + 4`, robust list); `copy_out` faults on overflow | M-F6 |
| `e43dbe8b082` | segments copy pages and count writes; attachment syncs read only when someone else wrote | M-F7 |
| `d4c339f1627` | `ipcget` check order; `mq_unlink` sticky-root owner check | L-F1, L-F7 |
| `cd242429a3d` | ITIMER_REAL re-arms when SIGALRM is dequeued; `setitimer` 0 forgets the interval | L-F4 |
| `52098cb3951` | registrations dropped during a wake are deferred until the queue lock is released; `WaitEntry` contract | L-F5 (and part of L-X4) |

Tests: `034c3fa` adds `mmap-mremap`, `thr-mutex-pi-signal`, `ipc-robust-exit-group`, and changes
`thr-atomic-wait-signal` (an untimed wait goes on after any handler).

What was checked: a release build at `52098cb3951`, and suite filters with the tests at `034c3fa`:
`base-` 21 pass, `mmap-` 4, `thr-` 15, `memfd` 3 (445 s; check that this was only machine load),
`ipc-` 6 pass and 1 failure. The failure is `ipc-robust-exit-group`, which does not compile: it
calls `exit` without `#include <stdlib.h>`.
Not checked: crate tests, clippy, fmt, a full suite run, the Open POSIX/LTP futex, mremap and IPC
tests that may now pass. The wasmer branch's `tests/wasixv2` pointer was never bumped.
WP3 findings not done: L-F2 (`msgctl(IPC_SET)` `expunge_all`), L-F6 (`TFD_TIMER_CANCEL_ON_SET`),
L-F8 (`semtimedop` precedence), L-F9 (`sched_setaffinity` length).

**How to finish.**

1. Rebase: `git checkout -b wasixv2/review-futex-mm-2 origin/wasixv2/review-futex-mm` and
   `git rebase --onto <feat/wasixv2 after train 5> origin/wasixv2/v1-sigterm`. Tests:
   `git rebase --onto origin/main 7795723` on a copy of `review-futex-mm`.
2. Fix the test: add `#include <stdlib.h>` to `suite/ipc-robust-exit-group/main.c` (amend into
   `034c3fa` or add a follow-up commit).
3. Gates: `plan/gates.sh <wasmer> tests`, `clippy`, `fmt`. Also `cargo test -p wasixv2-wait`.
4. `plan/gates.sh <wasmer> release`, then `plan/suite.sh … copy -- thr-` and `ipc- mmap- memfd
   base- sig-` filters, then the upstream filters `futex`, `pthread_mutex`, `mremap`, `shm`, `msg`,
   `sem`, `mq_`, `setitimer`, `robust`. List the newly passing tests from the `ef` diff for the
   bless step, without editing `expected-failures/` yourself.
5. Optional, as time allows: L-F2, L-F6, L-F8, L-F9 as separate commits with kernel tests.
6. Push as new branches (`wasixv2/review-futex-mm-2`, tests `review-futex-mm-2`), then merge into
   train 6.

### WP5 + WP6 host network bridge and Unix sockets (`wasixv2/review-net`)

State: [state/review-net.md](plan/state/review-net.md). One commit, `70c542fa87b`
(19 files, +2794/−479), covering H7, H8, H9, M-NET1..6 and L-NET1..7 (see the review).

**Where it stopped.** Passed on its base: cargo test (vnet 43, vnet-host 4+12, kernel 188),
clippy with the lint flags, fmt, and the suite filter `net` (195 pass, including the 5 new `netfix-*`
tests at tests `45b0716`). Not run: Open POSIX/LTP socket tests, the full suite. The submodule
pointer was never bumped. The agent was splitting the commit when it stopped.

**How to finish.**

1. Split `70c542fa87b` into logical commits, in the planned order (state file, step 1). The prepared
   material is in [plan/review-net-split/](plan/review-net-split/): `s0.txt`/`summary.txt` list
   every zero-context hunk, `hunks.py <diff> <json>` regenerates the hunk index from
   `git diff -U0 bdcdb958bef 70c542fa87b`, and the `*_block.rs`, `*_final.rs` and
   `reactor_tests.rs` files hold the extracted test modules and the final files. Each stage must
   build and pass its tests. The tree after the last commit must equal `70c542fa87b`'s
   (`git diff 70c542fa87b` empty). The split is optional: the coordinator asked for it, but one
   commit is acceptable if the split costs too much.
2. Rebase onto train 5 + WP3 (no conflicts expected) and rerun the vnet, vnet-host and kernel
   tests, clippy and fmt.
3. Release build. Run `plan/suite.sh … copy -- net` and the upstream filters `socket`, `accept`,
   `bind`, `sendmsg`, `recv`, `getsockopt`, `setsockopt`. Report the tests that newly pass.
4. Smoke: run-all.sh. sws over `--publish` exercises H8 (options before connect) and M-NET1
   (accept gating).
5. Leave for later: the DESIGN "ping" sentence (WP11), the pump holding `StreamState` across host
   syscalls, `SO_*BUFFORCE` for IP sockets, Unix `SO_RCVLOWAT`, netlink unicast.

### WP9 signals (`wasixv2/review-signals`)

State: [state/review-signals.md](plan/state/review-signals.md). One fix commit `8d686147337`
(M-P4, M-P6, M-P7, M-P9 deferral, L-P1, L-P3, L-P4, L-P5) and a `wip:` tests bump.

**Where it stopped.** 195 kernel unit tests pass. Clippy and fmt are clean on kernel, exec,
syscalls, wasix1 and wasixv2. Not run: the exec, wasix1 and syscalls crate tests, and any suite
test. The 5 new guest tests at tests `0a9779e` (`sig-altstack-autodisarm`, `sig-altstack-inherit`,
`sig-rlimit-sigpending`, `proc-pdeathsig`, `proc-vfork-parent-exit`) have never run and may need
fixes.

**How to finish.**

1. Split `8d686147337` into one commit per finding, each with its kernel tests in
   `kernel/src/tests/signal.rs` (requested; optional like WP5's split). Drop `1f3950f419d`.
2. Rebase onto train 5 + WP3 + WP5+6 and resolve the two conflicts as "Merge order" describes.
3. Run `plan/gates.sh <wasmer> tests` (the exec, wasix1 and syscalls tests too), `clippy` and `fmt`.
4. Release build. Run `plan/suite.sh … copy -- sig-`, `proc-` and `thr-`, then the upstream filters
   `sigaltstack`, `sigqueue`, `kill`, `prctl`, `pidfd`, `signal`. Fix the new guest tests until they
   pass. Rename the tests commit without `wip:`.
5. Follow-ups, not in this train: a killable vfork parent (needs a `Control` method, so an ABI
   change in wasixv2-abi first, rules 6), `pidfd_send_signal` flags, `CLONE_PIDFD` (needs the
   pidfd written by `GuestAction::Fork`).

### Landing train 6

1. tests: a branch `train6` from `main` with `034c3fa'` (rebased, with the include fix),
   `45b0716` and `0a9779e'`.
2. wasmer: a branch `wasixv2/train6` from `feat/wasixv2`. Cherry-pick the finished WP3 commits,
   then WP5+6, then WP9, resolving as described. Add a last commit pointing `tests/wasixv2` at
   tests `train6`.
3. Run all gates (`plan/gates.sh <wasmer> all` and `cli`), then a full suite with `copy`. The
   bless agent reviews the `ef` diff, moves newly passing tests off the lists, checks flaky ones
   by rerunning them, and commits the bless on tests `train6` as `chore: re-bless for …`. Rerun
   the full suite unblessed, which must show 0 unexpected. Run the smoke tests.
4. libc2: a commit bumping `wasixv2/tests` to the final tests commit.
5. Land fast-forward in the order libc2 `main`, tests `main`, wasmer `feat/wasixv2`.

## 3. Review work packages not started

The senior review of `bdcdb958bef` and the train 5 branches is [plan/v2-review.md](plan/v2-review.md):
no critical findings, 12 high, ~35 medium, ~50 low. Its work packages (§3) and their status:

| WP | Scope | Findings | Status |
| --- | --- | --- | --- |
| WP1 | exec/de_thread correctness | H3, H4, M-P1, M-P2, M-P3, M-P5, L-P2 | landed (`e595aea505f`); M-P8 in train 5 |
| WP2 | loader as binfmt registry, exec cost | H10, M-CLI2, M-CLI3, M-W5, L-CLI3, L-CLI4, L-FS11 | landed (`783b2cfcf2f`); H6, M-P8 in train 5 |
| WP3 | futex + mm | H2, M-F1..7, M-SIG1, L-F* | landed (train 6) |
| WP4 | fs/vfs | H1, M-FS1..5, L-FS1..10, L-FS12 | landed (`9c147516805`); M-FS6 in train 5; L-FS12 partly |
| WP5/6 | host network bridge, Unix sockets | H7, H8, H9, M-NET1..6, L-NET1..7 | landed (train 6) |
| WP7 | wasix1 correctness | H5, H11, H12, M-W1, M-W2, M-W4, L-W* | landed (`783b2cfcf2f`) |
| WP8 | syscall hot-path performance | M-W3, M-X2, M-X3 | landed (`9d04b0b7899`); v2 import filter needs codegen |
| WP9 | signals | M-P4, M-P6, M-P7, M-P9, L-P1, L-P3..5 | landed (train 6) |
| WP10 | facade/CLI | M-CLI1, M-CLI4..6, L-CLI* | landed (`11d0ae2781b`); real Ctrl-C needs job control |
| WP11 | robustness | M-X1, M-X4, L-X1, L-X4, L-NET8, L-F11 | landed in train 7 (lock sweep included) |
| WP12 | libc2 vfork | M-LIBC1 | done in train 5 |

Each WP is one agent on its own branch from `feat/wasixv2` after train 6, with kernel tests and
suite tests for every fix, the gates, and the filtered suite runs. The review gives every finding a
location, the Linux reference and the fix.

- **WP1 exec/de_thread.** Wait for the siblings before `exec_commit` (await the `exec_wait`
  "alone" predicate in `sys_execveat`; when the caller is not the leader, commit from the leader's
  side). EAGAIN on an exiting group, and drop `tree.images[task]` in `forget_task`. Tolerate a
  taken-over task in `signal_return`/`TaskSignals::with` (H4 host panic). `exec_reset` clears
  `saved`/`notify`. Abort `SwitchTask`/`Fork` children when an exit is queued. In `may_signal`,
  copy the sids before locking (ABBA, M-P1). argv/env as bytes end to end. Do it first: H3/H4
  are correctness and panic findings. It touches train 5's `kernel_execve`/`exec_commit` and WP9's
  pdeathsig line in `exec_commit`, so it starts after train 6. Do it before WP2, which changes the
  same exec path.
- **WP2 rest.** `Loader::load(Image { bytes, identity })`. Key the cache on identity or the raw-bytes
  hash, and instrument only on a miss. Probe the first 256 bytes, with no whole-file copy before the
  format check. Compile once in `wasmer run`. `run_file` = `kernel.exec` + the one image loop. Seed
  `CachedLoader` with the CLI's engine. Move the v1 command defaults onto the command file
  (`Arc<RwLock<Commands>>`). Then fix the loader/DESIGN docs and cap the `from_files` depth. Also
  drop the CLI's `check_command_atom`, which the kernel probe now covers. After WP1. It unblocks
  LTP `cp`/`mv` (exec cost) and shell loops.
- **WP4 fs/vfs.** `RESOLVE_IN_ROOT` (H1, a sandboxing primitive) first. Then SIGPIPE on partial
  pipe writes, a reusable I/O scratch, full-buffer stream reads, the `F_SETLK` close race, epoll
  bulk copy-out with requeue, and the lows. Independent of WP1/WP2; it can run alongside them.
- **WP7 wasix1.** Scratch window bounds (H5: host panic) and clamping v1 memory below
  `SCRATCH_BASE`. Default `/etc/resolv.conf` from the host nameservers, and a fast-failing loopback
  fallback (H11: no DNS for v1 programs). Killable threads after `pthread_create` (H12). Copy the
  fs/net tables in `ProcState::fork`, fix `tty_set` line_feeds, the bogus-callback fallback, and
  drop epoll/timeout records on close. Document or re-home the preopen fd-3 behaviour. It touches
  `wasix1/calls/proc.rs` like WP2's M-W5, so land WP2's command-defaults move first, or have one
  agent do both. Before WP8.
- **WP8 performance.** wasix1: cache the pid, a reusable scratch, `nbytes = 0`, one `rt_sigaction`
  pass, imports filtered by module, an errno table. exec: generated imports filtered by
  `module.imports()`, and an unboxed guest-copy fast path. Measure with a `getpid`/`read`
  microbench before and after. After WP7 and WP2.
- **WP10 facade/CLI.** Ctrl-C counting from the kernel's answer; script dirs auto-mounted, `--use`,
  a default `PATH`/`HOME`; mount packages once; one tokio `Handle` + `LocalSet`;
  `Machine::shutdown()`. After WP2 (`run_file`, `machine.rs`). The real Ctrl-C fix needs job
  control (gaps).
- **WP11 robustness.** Poison-tolerant locks (311 `lock().unwrap()` sites; `parking_lot` or a
  helper); report `instrument()` failures; `settle` clears `unwinding` on error; DESIGN `ping`. The
  lock change touches every kernel file: land it in a quiet window between trains, as a mechanical
  commit of its own.

## 4. LTP shell tests

v2 builds of bash 5.3, coreutils 9.12, grep, sed, gawk, findutils, diffutils, which, util-linux
2.41.2 (mount, umount, mountpoint, findmnt, unshare, nsenter, setsid, taskset, renice, uuidgen,
ipcs, ipcrm, hexdump, flock, getopt), gzip, bzip2, tar, cpio, Info-ZIP unzip, procps-ng (ps,
sysctl, free, pgrep, pkill, pidof, kill, ...), psmisc (killall, fuser) and LTP's shell library
exist; the build scripts are in [plan/userland/](plan/userland/) (`build-all.sh`, one
`<pkg>/build.sh` each, `package/gen.sh` for a local webc v3 package, `tests/smoke.sh`,
`tests/run-ltp.sh`, `tests/ltp-sweep.sh`, minimal repros in `tests/repros/`).
They build against the asyncify sysroot for the host `wasm32-unknown-linux-musl`. With train 9's
CLI, `tar01`, `cpio01`, `unzip01` and `gzip01` pass as well; `fs_bind*` need the mount syscalls
(branch `wasixv2/mounts`), `unshare01` new namespaces, and `killall` `pidfd_send_signal` on a
`/proc/<pid>` directory.
iproute2 7.3.0 (`ip`, `ss`) and net-tools 2.10 (`ifconfig`, `route`, `netstat`, `arp`,
`hostname`) are built too, for the network tests, which configure interfaces, addresses, routes,
veth pairs and namespaces (wasmer branch `wasixv2/net-config`). By hand on that branch (root, a
`/lib/modules/<release>/modules.builtin` naming veth and dummy, `KCONFIG_PATH`): `netns_sysfs`,
`netstat01` and, over a veth pair between namespaces, the TCP runs of `dccp01`/`sctp01` (both
families) pass; tst_net.sh's two-namespace setup works for every net.* test. iputils 20240905's
`ping` (and `ping6`) is built too (needs meson on the host), for the ICMP sockets of wasmer branch
`wasixv2/net-followups`, which also has multicast memberships, `IP_BIND_ADDRESS_NO_PORT`, socket
inodes in `/proc/net/tcp` and, in the default root, `/var/run` and `/lib/modules/<release>`. By
hand there (root, `KCONFIG_PATH`): all 16 `netns_breakns`/`netns_comm` tests and `netns_sysfs`,
all of net.multicast (`mc_cmds`, `mc_commo`, `mc_member`, `mc_opts`), `ping601`/`ping602` and
`bind_noport01` (both families) pass. Still missing: `ipneigh01` (puts `-I` after the target,
which musl's getopt does not permute), `sendfile01`/`sendfile601` (LTP's `datafiles` are not in
the suite's install; `ss -ltp` now finds the server), `tc` (not built: no queueing disciplines),
the other link kinds (vxlan, vlan, gre, ...), CONFIG_TUN, and the tools tcp_cmds names (arping,
tcpdump, telnet, traceroute, lsmod, ...). The runner needs `KCONFIG_PATH`.
Not built: binutils and file (`file01`, `nm01`, `ld01` inspect ELF objects and `/bin/sh`, which
are wasm modules here), `ldd` (no dynamic loader). With train 5, LTP's
`tst_test.sh` runs: du01, ln, mkdir, wc01, which01, tst_errexit and timeout01/02 pass; cp/mv pass
but need the exec image cache (WP2).

Next:

- Move the build scripts to a repository of their own (to be decided) and publish the package.
- Runner change in wasixv2-tests (owned by the test-system team): run shell tests as
  `bash -c "<runtest line>"` with the userland mounted at `/bin`, the test shell not pid 1
  (`tst_timeout_kill` rejects pid 1), and each test in its own process group.
- `shell_loader*` need memory shared between processes, which wasixv2 fork can't provide.

## 5. Other known gaps

- Upstream tests that fork are built for the exception-handling configuration and fail with
  ENOSYS (221 Open POSIX, 227 LTP). Building them for the asyncify configuration is a runner
  change.
- `splice`/`vmsplice`; `/proc/sysvipc/*`, `/proc/sys/kernel/shm*`, `/proc/self/ns/*`; fcntl leases,
  `F_GETOWN_EX`.
- The review's top gaps (§5): job control and a controlling terminal; process accounting, CPU time,
  pid namespaces; inbound UDP and the socket error queue; a dentry cache and `mount` syscalls.
- v1 compat: v1 command defaults don't apply through symlinks or as a
  `#!` interpreter (WP2); wasix1's C tests need a v1 wasixcc (`WASIX1_TEST_CC`).
- wasm traps on calls through mismatched function pointers (K&R-style thread functions in
  Open POSIX, `prctl08`).

## 6. Toolchain, gates, suite, smoke

The scripts in `plan/` take their paths from the environment. [env.sh](plan/env.sh) documents each
one. By default everything lives in `$WASIXV2_TOOLS` (`~/wasixv2-tools`).

Toolchain from scratch:

1. Rust: the repository's toolchain for wasmer. For the sws-v2 smoke binary only, also nightly
   `1a98b1e13` (2026-08-07) with `rust-src`.
2. Host tools on `PATH`: cmake, ninja, binaryen (`wasm-opt`, version 129 was used), autoconf
   (2.73), automake (1.18), libtool (2.6), m4, pkg-config, wasm-tools, curl. Set
   `PKG_CONFIG_ACLOCAL` to pkg-config's `share/aclocal` if `aclocal` does not find `pkg.m4`.
3. LLVM: `git clone -b wasix-22.1.8 https://github.com/wasix-org/llvm-project $WASIXV2_TOOLS/llvm-project`, then
   `cmake -G Ninja -S llvm -B build -DCMAKE_BUILD_TYPE=Release -DLLVM_ENABLE_PROJECTS="clang;lld" -DLLVM_TARGETS_TO_BUILD=WebAssembly`
   and `nice -n 19 ninja -C build -j3`.
4. wasixcc: `plan/ensure-wasixcc.sh` (clones wasix-org/wasixcc `feat/wasixv2` and builds it into
   `$WASIXV2_TOOLS/bin`). Then `source plan/env.sh`.
5. libc2 (private repository: needs access): clone it into `$WASIXV2_TOOLS/libc2`, check out the
   branch under test, and run `git submodule update --init wasixv2/abi`. Then run
   `make wasixv2-sysroot` and after it `make wasixv2-sysroot-asyncify`. They build
   `build-wasixv2/sysroot-wasixv2` and `sysroot-wasixv2-asyncify`, which `WASIXCC_SYSROOT_PREFIX`
   points at.
   The sysroot build runs one job per core whatever `-j` says: use
   `MAKEFLAGS= OMP_NUM_THREADS=3 nice -n 19 make …` on a shared machine. The sysroot has no libc++,
   so C++ programs can't target `wasm32-wasixv2` yet (a gap).
6. CLI integration tests: `wasixccenv download-sysroot v2026-05-12.1` (with `WASIXCC_SYSROOT_PREFIX`
   unset, or it lands in libc2's build directory) (the released v1 sysroot,
   in `~/.wasixcc/sysroot-v2026-05-12.1`).
7. wasmer: check out the branch, then
   `git submodule update --init lib/napi lib/wasixv2/codegen tests/wasixv2 wasmer-test-files` (cargo needs
   `lib/napi` for workspace metadata; the CLI `--test run` tests need `wasmer-test-files`) and `git -C tests/wasixv2 submodule update --init` (the
   external suites: LTP, Open POSIX, libc-test; without them the runner skips those suites).

Gates and runs (rules 8 and 12: `-j3`, `nice -n 19`, one heavy command at a time):

- `plan/gates.sh <wasmer> tests|clippy|fmt|release|all` and `plan/gates.sh <wasmer> cli`: crate
  tests of all wasixv2 crates, clippy with the Makefile's lint RUSTFLAGS, fmt, the release CLI, and
  the CLI `--test run` tests. Logs go to `<wasmer>/target/gates/`, each ending in `EXIT <status>`.
  For quick iteration:
  `nice -n 19 cargo check -j3 -p wasixv2 -p wasixv2-kernel -p wasixv2-exec -p wasixv2-wasix1 -p wasmer-cli --features wasmer-cli/cranelift`.
- `plan/suite.sh <wasmer> <libc2>/build-wasixv2 <out> [copy|bless] [-- FILTER]`: the suite with the
  release CLI, 3 threads on 4 cores (`taskset -c 0-3`; LTP's build uses every core otherwise),
  60 s per test. A full run takes about 25 minutes. `copy` blesses a copy to diff. `bless` rewrites
  the tree's lists, which only merge/bless agents do. `flaky.txt` and the hand-written
  `## flaky:` reasons are kept by hand.
- Smoke: build `plan/smoke/sws-v2/build.sh` (static-web-server 2.44.0 for `wasm32-wasixv2`; see
  its README), then `SWS_WASM=… plan/smoke/run-all.sh <abs wasmer>`. It runs the sws-v2 HTTP
  checks (28) and the v1 registry packages, which need network access to the registry.
  `plan/smoke/train5-extras.sh <wasmer>` checks SIGTERM of v1 servers, `wasmer/bash` with commands
  in `/bin`, and the userland package. Set `USERLAND` to a built `plan/userland`.

Orchestration material, copied into `plan/`. The records keep the original machine's paths: read
them as descriptions, not locations.

| File | What |
| --- | --- |
| [rules.md](plan/rules.md) | binding rules for every agent (scope exclusions, quality bar, isolation, ABI, pushes, gates, shared-machine limits) |
| [v2-review.md](plan/v2-review.md) | senior review: findings, work packages, lock-ordering model, gaps |
| [integration.md](plan/integration.md) | merge log of trains 4 and 5 |
| [state/](plan/state/) | the STATE files of review-net and review-signals |
| [review-net-split/](plan/review-net-split/) | prepared split of WP5+6's commit |
| [env.sh](plan/env.sh), [ensure-wasixcc.sh](plan/ensure-wasixcc.sh) | toolchain environment, wasixcc build |
| [gates.sh](plan/gates.sh), [suite.sh](plan/suite.sh) | gates and suite runs, as train 5 ran them |
| [smoke/](plan/smoke/) | `run-all.sh`, `sws-v2-test.sh`, `v1-packages.sh`, `train5-extras.sh`, the sws-v2 build kit |
| [userland/](plan/userland/) | v2 userland and LTP shell library build scripts |

## 7. Decisions

Decided by the user on 2026-10-27:

- wasix1 `0777` create mode (`f71e0b1a38b`, train 5): kept for now. A WP7 follow-up finds what v1
  bash asks for and matches that in the compat layer instead, which also fixes the exec bits on
  files created in mapped host directories (L-W4).
- `__linux__` for the `wasm32-wasixv2` target, together with `os = "linux"` for the Rust target:
  yes. A work package of its own after train 6 (wasixcc, libc2, the Rust target).
- DNS for v1 programs (H11): WP7 generates a default `/etc/resolv.conf` from the host nameservers,
  with a fast-failing loopback fallback. **TODO, deferred but definite:** an in-process DNS server
  for v1 programs.
- `--fpcast-emu`: no. Calls through mismatched function pointers keep trapping.
- libc2 visibility: deferred; machines with access build it locally.
- LTP's 905 root-only tests: run them as uid 0 (a runner change in wasixv2-tests).
- A Machine option to run an init process as pid 1, and a 64 KiB guard at address 0: both, as a
  work package after WP11.
