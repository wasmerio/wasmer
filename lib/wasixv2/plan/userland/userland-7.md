# Userland 7: follow-ups from the real-program sweep

Runtime branch `wasixv2/v10fu` starts at v9progs `b178e7a7b16`. Runtime head is `9d58b5317da`: DONT_MASK
`01112d3f6e6`, exit-check race `12b483ba18b`, device splice `2bc629353b1`, and
blocking splice reads `9d58b5317da`. Tests `v10fu` is `9c3eace`, libc2 `v10fu` is `e8694467` (ABI remains
`07cd32e`). Build/run artifacts stay outside git under `~/wasixv2-tools/userland-7`.
Initialize this directory by copying the source builds, `bin`, `out`, and PostgreSQL
installation from userland-6; retain original v9 artifacts. See userland-6.md for
pinned upstream source downloads and initial build recipes.

The focused follow-up driver uses the original workflows and tools:

```sh
bash tests/followups-7.sh /absolute/release/wasmer overlay
bash tests/followups-7.sh /absolute/release/wasmer busybox
PANIC_ITERATIONS=20 bash tests/followups-7.sh /absolute/release/wasmer panic
bash tests/followups-7.sh /absolute/release/wasmer postgres
```

`USERLAND7` selects the new build root; `USERLAND6` selects the original build
root for any inherited inputs. Programs run at nice 19 on CPUs 4–7, compiler
threads/jobs 3. The panic driver restores the original coreutils test order,
including mktemp's unsupported strace-injection test, solely to reproduce the
host crash. Each iteration preserves output and enables `RUST_BACKTRACE=full`;
unsupported injection is not reported as validated behavior. No full WASIX suite
is invoked.

FUSE now advertises and negotiates DONT_MASK. VFS creation metadata retains the
umask; FUSE handles creation-mode/ACL inheritance in the daemon, masks mode only
without DONT_MASK, and sends the mask for CREATE/MKNOD/MKDIR. MKDIR excludes the
file-type bits. Linux references: fs/fuse/inode.c:fuse_send_init/process_init_reply
and fs/fuse/dir.c:fuse_create_open/fuse_mknod/fuse_mkdir. Failing-first kernel wire
coverage includes all these operations, negotiated ACLs, and the ENOSYS CREATE
fallback. The 43 FUSE tests, 41 kernel fs tests, affected VFS driver tests,
Makefile-flags clippy and formatting pass. Program validation follows below.

## Busybox's twelve classified failures

| Failure | Source evidence / disposition |
| --- | --- |
| awk gsub erroneous word start match | editors/awk.c:awk_sub explicitly describes this bug in its branch without REG_STARTEND. Musl does not offer that BSD extension. Upstream applet limitation; no kernel or regex workaround. |
| cpio lists hardlinks | testsuite/cpio.tests explicitly marks the missing names as an upstream known bug. |
| date-timezone | Real libc2 compatibility bug: strptime %z rejected ISO-8601 Z. Fixed with a failing-first fixture; glibc time/strptime_l.c:__strptime_internal accepts Z as UTC. Numeric offsets and unrelated fields are regression controls. |
| hostname-i-works | Test compares net-tools' IPv6 result ::1 to Busybox's IPv4 127.0.0.1. Busybox uses gethostbyname and inet_ntoa (networking/hostname.c). |
| mount -o remount,mand | Test requires a loop block device and Minix filesystem; neither is provided by this machine. |
| mount RO loop | Requires a loop block device and ext2 filesystem, also unavailable. |
| sed embedded NUL | testsuite/sed.tests explicitly guards this known bug with SKIP_KNOWN_BUGS; applet C-string handling. |
| sed NUL in command | Same explicit upstream known-bug guard. |
| sed nonexistent label | testsuite/sed.tests explicitly documents missing-label validation with no input as an incompatibility and guards it as a known bug. |
| start-stop-daemon with both -x and -a | Test explicitly rejects a multicall /bin/false. The fixture's coreutils is multicall and interprets argv[0]=qwerty. |
| strings-works-like-GNU | GNU strings reference program missing from the target fixture; Busybox strings itself runs. |
| unzip (subdir only) | zip fixture generator missing from the target environment. |

All failures remain visible; no exclusion/expected-failure list is changed.
Rebuild Busybox against the fixed libc2 sysroot before rerunning the focused
applets. In an isolated copy of busybox-src, remove only `busybox` and
`busybox_unstripped`, then invoke the same make command/wrappers as userland-6.sh.
Install the new binary into both `out/bin` and `bin` of userland-7.

## PostgreSQL dependency boundary

`postgres-plugins-7.sh` invokes the ordinary upstream Snowball and PL/pgSQL
installation targets using the existing PIC configuration. The current wasixcc
rejects the first object with `PIC is not supported for wasm32-wasixv2 yet`.
This confirms that missing dict_snowball is a compiler/dynamic-loading capability
boundary rather than a missing install command. The target compiler also rejects
-shared/-pie explicitly. No compiler or runtime linking code was changed.

PostgreSQL 17.5 has no supported configure/initdb option to omit these required
catalog components. Removing snowball_create.sql contents, skipping initdb stages,
or substituting C functions would not produce a valid standard installation.
Completing initdb and running pgbench therefore requires the excluded linking
team's supported v2 PIC compiler/sysroot and loader, followed by the ordinary
plugin recipe and existing postgres workflow. The existing static server reaches
bootstrap and exercises System V shared memory and semaphores; that is not a
completed initialization or a pgbench result.


## Final validation

| Follow-up | Result |
| --- | --- |
| fuse-overlayfs 1.15 / libfuse 3.17.4 | PASS existing overlay workflow: copy-up preserves lower data, whiteout hides the removed file, new-file creation reaches upper, normal unmount and daemon exit complete. |
| Busybox 1.37.0 | Relinked unchanged source against libc2 e8694467. Focused failing-applet set: 198 PASS / 11 FAIL / 1 SKIP. date-timezone now passes; all other failures match the classifications above. This does not claim a new complete Busybox-suite run. |
| Intermittent host panic | Reproduced on original-order coreutils iteration 2 with full backtrace, then deterministic kernel RED. Fixed exit-check/reap race; twenty complete original-order workflow iterations on 2bc629353b1 contain no host panic. Later change 9d58b5317da affects FUSE only. |
| PostgreSQL 17.5 | initdb bootstrap PASS, post-bootstrap FATAL on missing dict_snowball. Ordinary plugin recipe fails unsupported v2 PIC. pgbench NOT RUN. No unsupported catalog omission or linking patch. |
| sshfs / GLib (optional) | Not pursued: inherited GLib build requires PCRE2 and excluded libffi work. |

The FUSE run exposed two further Linux bugs after INIT succeeded. libfuse selects
splice by protocol version; the device now implements copying splice transport.
Linux fs/fuse/dev.c:fuse_dev_splice_write consumes exactly the requested bytes
before validating the reply, but leaves a pipe with insufficient bytes unchanged;
regression coverage includes both cases. A blocking device read must remain
blocking when its output pipe has O_NONBLOCK: fs/splice.c:splice_file_to_pipe
uses nonblocking flags for pipe room, while fs/fuse/dev.c:fuse_dev_do_read checks
the source file's flags. That behavior has its own failing-first regression.

The panic was take_pending -> ptrace_pending_exit -> task_signals. Once any
process had used ptrace, a later exit check performed an infallible second lookup
after take_pending released its task-table lock; reap could win in between.
Snapshotting signals/process with a fallible lookup preserves the existing
DeadTask unwind action. Linux kernel/exit.c:do_exit orders ptrace EXIT before
exit_notify/release_task; an already unhashed task cannot acquire a new EXIT stop.
No kernel/exec contract or guest ABI changed.

Final gates: **310 distinct Rust tests** (45 FUSE, 31 ptrace, 56 task, 41 kernel
fs, 137 across affected VFS driver crates), all-target clippy using the exact
Makefile lint flags for touched crates, and cargo fmt --all --check PASS.
Both libc2 sysroots build; the strptime fixture passes with EH and asyncify.
Release CLI builds PASS, including the final FUSE correction. No full WASIX or
workspace test suite was run. All heavy commands were serialized, nice 19,
CPUs 4–7, jobs 3, incremental disabled, with release builds in systemd user units.

PostgreSQL's final trace has 12 shmget, 10 shmat, 12 semget, 182 semctl, and 12
semop call entries before the verified plugin blocker. These counts establish
IPC exercise, not completed initialization. The linking team must supply a
supported v2 PIC compiler/sysroot and loader before the plugin recipe, initdb,
and pgbench can finish.

Machine-local evidence and handoff: `~/wasixv2-orch/logs/v10fu/STATE.md`;
`traces/panic-before.log`, twenty `traces/panic-after/panic-N.log` files, original
splice failure traces, and final overlay/PostgreSQL traces. The final tested
CLI is archived at `logs/v10fu/bin/wasmer` with source manifest and SHA256SUMS.
Source-only recipes are committed; no prebuilt Wasm or expected-failure edits.
