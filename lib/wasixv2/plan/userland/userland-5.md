# Real-program validation kit

`userland-5.sh` builds pinned upstream sources into
`${USERLAND5:-~/wasixv2-tools/userland-5}`. It uses `common.sh` and the libc2
asyncify sysroot for every program. Nothing generated belongs in git.

Use the environment from `../env.sh` (including the local LLVM and Binaryen
paths). Build one package at a time:

```sh
nice -n19 taskset -c 4-7 bash userland-5.sh liburing
nice -n19 taskset -c 4-7 bash userland-5.sh keyutils
nice -n19 taskset -c 4-7 bash userland-5.sh util-linux
nice -n19 taskset -c 4-7 bash userland-5.sh iproute2
nice -n19 taskset -c 4-7 bash userland-5.sh seccomp
nice -n19 taskset -c 4-7 bash userland-5.sh core
nice -n19 taskset -c 4-7 bash userland-5.sh fuse
```

The util-linux build includes private libcap-ng. The FUSE build includes
private zlib and libfuse. Their Autoconf function probes use incompatible C
declarations on wasm; the script verifies correctly prototyped links before
supplying the corresponding configure cache answers.

The runner combines the source-built userland-4 shell/tools with userland-5.
Run exactly one workflow, using a release CLI built in a systemd user unit:

```sh
bash tests/run-programs-5.sh /absolute/path/to/wasmer network
```

Workflows: `network`, `namespaces`, `privileges`, `keys`, `seccomp`, `core`,
`fuse`, `liburing`, `liburing-timeout`, `liburing-followups`, `libaio`,
`libaio-setup`. The core workflow expects SIGABRT, invokes host
`readelf --notes`, then checks the ELF32 notes and memory independently.
The FUSE workflow also needs host `mksquashfs`. The seccomp executable is
Linux's `samples/seccomp/dropper.c`, taken from `${LINUX_SOURCE:-~/wasixv2-tools/linux}`;
syscall 56 is the pinned WASIX ABI's `openat`, audit architecture riscv32.

The liburing workflow runs the upstream `make -C test runtests` recipe with
ten explicitly selected tests, through guest bash. It does not run the full
suite. `liburing-timeout` runs only upstream `timeout.t`, with its built
`exec-target.t` helper available for the fork/exec cancellation case.
The libaio recipe similarly selects seven harness cases.

## Results on train 18

Validation baseline: wasmer `21587f4e504`, libc2 `1df9bb80`, ABI `07cd32e`.
Detailed build/run logs and handoff notes are in
`~/wasixv2-orch/logs/v3progs/STATE.md` on the validation machine.

| Program | Result | Evidence or blocker |
| --- | --- | --- |
| iproute2 7.3.0 | Works | `ss` reports a live listener and owner; netns add/configure/exec/delete |
| util-linux 2.41.2 | Works | unshare/nsenter user, pid, mount, net, time workflows; setpriv credentials and no_new_privs |
| keyutils 1.6.3 | Works | Session keyring, add/read/describe/update/show/unlink/clear |
| Linux seccomp dropper | Works | openat returns EACCES in exec'd ls; unfiltered ls succeeds |
| Core file tooling | Works | readelf and independent reader verify NT_AUXV, NT_FILE offsets/paths, memory marker |
| squashfuse 0.5.2 / libfuse 3.17.4 | Works | Mount/read/list/symlink/nested compressed 128 KiB file/unmount/daemon exit |
| libaio | Build blocked | ELF `.symver` directives in `src/syscall.h`; linking code is excluded from this package |
| strace 6.17 | Build blocked | `configure.ac` rejects wasm32; requires an architecture port before either `-f` workflow |
| liburing 2.12 | Fails selected cases | See below |

liburing's nop, eventfd and open-close tests pass on the baseline. Upstream
`timeout.t` passes on `wasixv2/v-progs` after two failing-first runtime fixes:

- `f652976a3e6`: return from CQ waits on timeout expiry below `min_complete`,
  following Linux `io_uring/io_uring.h:io_should_wake`.
- `0bc36eb981f`: queue removed timeout completion as task work after the remover's
  inline completion, following `io_uring/timeout.c:io_timeout_cancel`.

The final runtime passed 73 filtered kernel io_uring tests, all-target kernel
clippy with Makefile lint flags, `cargo fmt --all --check`, a release CLI build,
and unmodified upstream `timeout.t`, including its fork/exec helper.

Remaining failures: read-write and socket
tests request SQPOLL, which this kernel explicitly rejects. Poll's
disabled-DEFER_TASKRUN test exposes delayed wake processing. Statx, unlink
and mkdir expect EFAULT at `0x1234`, inside the deliberately readable low
guard. Six selected upstream tests remain failing for these reasons.

Follow-up packages: a strace wasm architecture port; libaio's static wasm
build in coordination with the linking owner; io_uring wake/task-work
ordering; Linux-exact syscall reads of the low guard in coordination with
its documented LLVM workaround. No expected-failure lists were changed.


## V5 follow-up results

Validated on `wasixv2/v-uring` at `2c223d3165c`, based on `0bc36eb981f`.
The libc2/ABI pins remain unchanged. Detailed red/green logs and handoff state
are in `~/wasixv2-orch/logs/v5uring/STATE.md`.

| Selected upstream program | Result |
| --- | --- |
| liburing poll.t | PASS, including forked basic polling, deferred multishot readiness, disabled-ring polling and 65,536 self-polls (16 seconds total) |
| liburing statx.t / unlink.t / mkdir.t | PASS; pathname copies below the existing low guard return EFAULT |
| liburing timeout.t | PASS, including its exec helper |
| liburing read-write.t / socket.t | Still fail on SQPOLL setup returning EINVAL, an explicit kernel limit |
| libaio official harness cases/2.t | PASS inside the machine: all eight setup/context validation checks |
| Full libaio library / event-wait harness | Blocked by upstream's unconditional legacy io_getevents syscall; static .symver assembly is fixed |
| strace | Port plan recorded; no working wasm32 build or -f result claimed |

SQPOLL already returns exactly EINVAL, covered by the kernel setup unit test.
Liburing 2.12's `test/helpers.c:t_create_ring_params` does **not** skip that
result: it skips EPERM only for a non-root SQPOLL request. The kernel error
has not been changed to manufacture a skip. The six-test follow-up workflow
therefore exits 1 with exactly read-write.t and socket.t listed as failures.

Runtime fixes (each has a failing-first kernel regression and Linux citation):

- `4029e466cb7`: process poll wakeups on the submitting task's existing return hook.
- `d07c277311d`: fault shared pathname copies in the low guard, preserving Linux's
  NULL plus AT_EMPTY_PATH exemptions; guest loads and the exec contract stay as before.
- `c46a1196f39`: retain real registration-race callbacks without advertising idle polls
  as pending work (a regression found during validation of the first change).
- `6308f5e3a9f`: queue initial multishot readiness as task work, including deferred rings.
- `2c223d3165c`: detach one-shot registrations on their first wake and rearm after a
  spurious wake, keeping multishot registrations active.

Final checks passed: 78 io_uring tests, 27 filesystem tests, 27 mount tests,
all-target kernel clippy with Makefile lint flags, `cargo fmt --all --check`,
a systemd release CLI build and the selected program workflows above. No full
suite or expected-failure-list edits were made.

For libaio, use LLVM branch `wasix-org/llvm-project:wasixv2/wasm-symver`,
commit `568452b96`, through `LIBAIO_LLVM_LOCATION`. No LLVM release/tag was pushed.
The setup-only recipe uses a normal archive of the real io_setup object and
the harness's documented LIBAIO link-input option; no libaio source is patched.
See [libaio-time64.md](libaio-time64.md) for the independent full-build blocker
and proper time64-only port design. See [strace-wasm32.md](strace-wasm32.md) for
the syscall-info-only port plan, including scalar-slot widths and table differences.

Reproduce one workflow at a time using the archived release CLI or a build of
that runtime branch:

```sh
bash tests/run-programs-5.sh /absolute/path/to/wasmer liburing-followups
bash tests/run-programs-5.sh /absolute/path/to/wasmer liburing-timeout
LIBAIO_LLVM_LOCATION=/absolute/path/to/llvm/build \
  nice -n19 taskset -c 4-7 bash userland-5.sh libaio-setup
bash tests/run-programs-5.sh /absolute/path/to/wasmer libaio-setup
```
