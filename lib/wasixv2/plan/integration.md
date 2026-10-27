# wasixv2 integration log

Merge/landing log for the wasixv2 trains: what landed where, gate results, suite numbers,
smoke results and follow-ups. Append new entries at the end.

## 2026-10-27: train4 (fix-exec, exec Abi registry, WASIX v1 compat layer, single `wasmer run`)

Worktrees: wasmer `/home/theduke/dev/github.com/wasmerio/wasmer-train4` (branch
`wasixv2/train4-land`), comparison base `.../wasmer-train4-base` (origin/feat/wasixv2 7bad717d660),
libc2 `/home/theduke/dev/github.com/wasix-org/libc2-train4` (branch `train4-land`).
Logs: `/home/theduke/dev/wasixv2-orch/logs/train4-land/`.

### Pushed (all fast-forward)

| Repo | Branch | Range |
| ---- | ------ | ----- |
| wasixv2-abi | main | e2d007a..bcd7622 (fix-exec: Control's task lifetime hooks through Layered) |
| wasixv2-tests | main | aae6f73..b0861b8 (fix-exec tests e4b7c7b, sig-eintr-nanosleep f8b4f43, re-bless b0861b8) |
| libc2 | main | 6e604362..5d33ff2a (bump abi bcd7622 + tests b0861b8; C glue regenerated, no change) |
| wasmer | feat/wasixv2 | 7bad717d660..bdcdb958bef (train4's 21 commits + 4 below) |

### Review of train4

Checked the conflict areas: the kernel's per-task `mm` (kernel-mm-ipc) sits alongside fix-exec's
`work`/`restart` task fields and `de_thread`; the facade's `Machine::with_root` wraps
`MachineBuilder::root()` and `mount_root_lower*` rebuilds the kernel on an overlay root
before first use (EBUSY afterwards); exec fork carries the ABI payload and finds `__tls_base`
through the `Wasixv2Instance`. No broken merge found; builds, clippy and fmt were clean.

### Fixes added on top

- `12d33eec4c0` test(wasixv2): `a_busy_guest_takes_a_signal_at_a_preemption_point` was flaky
  (fails ~2/3 under load): one SIGTERM after 50 ms reached no process while the module still
  compiled. The sender now repeats until the run ends.
- `c25bb610bcd` fix(wasixv2-exec): asyncify-instrumented modules that do not export
  `__stack_pointer` (old wasi-libc builds; CLI test `list_cwd`) failed with "Missing export
  __stack_pointer". They now count as not instrumented (save/restore/fork via asyncify
  refused as for any uninstrumented module).
- `211fcc941cf` test(cli): 11 `--test run` tests whose guest is python/python (dynamically
  linked) are `#[ignore]`d with that reason, as the `--use` tests were.
- `bdcdb958bef` bump tests submodule to b0861b8.

### Gates

- Crate tests, all 22 lib/wasixv2 crates incl. wasixv2-wasix1: 484 passed, 0 failed
  (wasix1's C tests need WASIX1_TEST_CC/WASIX1_WASIXCC and were skipped).
- clippy with lint-packages RUSTFLAGS, `-D clippy::all`: lib/wasixv2 crates (`--all-targets`),
  wasmer-cli (cranelift), `--test run`: clean. `cargo fmt --all --check`: clean.
- CLI `--test run` (needs `WASIXCC_SYSROOT_PREFIX=~/.wasixcc/sysroot-v2026-05-12.1`, a v1
  sysroot; env.sh's v2 prefix breaks the C-compiling tests): 45 passed, 1 failed, 13 ignored.
  Base (origin/feat/wasixv2): 48 passed, 3 failed (same env).
  Delta vs base:
  - FAIL `issue_3794_unable_to_mount_relative_paths`: wasmer/bash runs `cat` from its
    coreutils dependency; dependency commands are not in /bin on the v2 path. Owned by the
    bin-commands work (wasmer-bin-commands).
  - ignored: 11 python/python tests (dynamic linking, excluded area), 2 `--use` tests.
  - renamed: `wasmer_run_wasix_wasm_c_api_guest` (failed on base) -> `..._is_refused` (passes).
  - new, passing: 7 v2/v1 tests from train4 (wasix1 module, wasm64 refusal, signals, traps,
    root volume, webc v2 migration).

### Suite (release CLI, tests b0861b8)

Environment: env.sh lacks autotools, so LTP's `make autotools` failed ("runner error" for all
of LTP) until automake/autoconf/libtool/m4 from /nix/store were put on PATH (see
`logs/train4-land/suite-bless.sh`). env.sh's `WASIXV2_TESTS_TIMEOUT_SECS=20` times out 5 tests
that need 20-30 s under load (nanosleep/10000-1, timer_settime/1-2, 2-1, semop05,
exec-exit-with-threads; each passes at the runner's default 60 s), so the bless uses 60 s.

Two unblessed 20 s runs, the 60 s bless run and a 60 s verification run; run-to-run differences
went to flaky.txt: aio_fsync/5-1, pthread_mutex_init/1-2, ltp/mm/mallocstress01,
ltp/mm/shm_test01. pthread_once/6-1 left flaky.txt (8/8 alone, 2 full runs). The six
"exit 139 after PASSED" flaky entries are reworded: `a_thread_func()` without parameters is an
indirect call type mismatch in wasm when the thread starts before exit (not a runtime bug).
Hand-written causes for killpg/6-1 and sem_unlink/5-1 restored after the bless.

Bless run: 2338 passed, 1233 failed as expected, 0 unexpected failures, 452 ignored.
Verification run against the blessed lists: 2344 passed, 1231 expected, 0 unexpected failures,
2 unexpected passes (mallocstress01, shm_test01 -> flaky.txt).

| Suite (current lists; flaky counted as ignored) | pass | expected failures | ignored |
| ----- | ---- | ----------------- | ------- |
| posix | 1328 | 465 | 83 |
| ltp | 464 | 738 | 370 |
| libc-test | 393 | 28 | 0 |
| own (suite/) | 160 | 0 | 1 |

New passes vs aae6f73: libc-test/functional/pthread_cancel, posix pthread_spin_lock/1-1, 3-1,
timer_create/10-1, 11-1, ltp fsx22, plus the new exec-* and sig-eintr-nanosleep tests.
No new failures. Cause changes: 4 posix tests "guest trapped: unreachable" -> exit 132
(traps as signals); prctl08 exit 1 -> 139 (calls a 1-parameter function with 2 arguments).

### Smoke (release CLI; scripts in /home/theduke/dev/wasixv2-orch/smoke/, `./run-all.sh <wasmer>`)

| Test | Result |
| ---- | ------ |
| v2 static-web-server (sws-v2 prebuilt, test.sh) | 28/28 |
| wasmer/static-web-server via --publish + curl | pass |
| phpix/phpix-85-32bit CLI script | pass |
| phpix/phpix-85-32bit `-S 0.0.0.0:8080 -t /app` via --publish + curl | pass |
| wasmer/coreutils `ls -la /`, `cat` | pass |
| syrusakbary/cowsay | pass |
| saghul/quickjs (`--command-name qjs`, -e and a script file) | pass |
| python/python | fails as expected (`env.__indirect_function_table`: dynamic linking) |

### Follow-ups

- Dependency commands in /bin (issue_3794): bin-commands branch.
- execve'd images are compiled anew each time (no module cache inside the machine):
  exec-exit-with-threads spends ~28 s on 200 spawns under load. bin-commands' `CachedLoader`
  covers this.
- env.sh: add autotools to PATH; reconsider the 20 s timeout for blessing (60 s default).
- wasix1 C tests (tests/c) need a v1 wasixcc via WASIX1_TEST_CC/WASIX1_WASIXCC to run.

## 2026-10-27: train5 (bin-commands, v1-sigterm, userland-fixes) — LANDED

Stopped on user request before landing. Nothing was pushed to feat/wasixv2 or any main branch.
The work is on backup branches only:

| Repo | Backup branch | Head | On top of |
| ---- | ------------- | ---- | --------- |
| wasmer | `wasixv2/train5` | 19 commits, last is the `wip:` tests bump | origin/feat/wasixv2 bdcdb958bef |
| wasixv2-tests | `train5` | 6405893 (5 cherry-picked tests) | main b0861b8 |
| libc2 | `train5` | 0006659a (`wip:` tests pointer bump) | main 5d33ff2a |

Worktrees: wasmer `/home/theduke/dev/github.com/wasmerio/wasmer-train5` (branch
`wasixv2/train5-land`), libc2 `/home/theduke/dev/github.com/wasix-org/libc2-train5` (branch
`train5-land`, both sysroots built at a838d039 in `build-wasixv2/`). Logs:
`/home/theduke/dev/wasixv2-orch/logs/train5-land/` (gates.sh, suite-bless.sh, extras.sh).

### Merged (cherry-picked in order onto feat/wasixv2)

- bin-commands, 8 commits. No conflicts. `fix(wasixv2-wasix1): files v1 programs create are
  executable` (0777 create mode) is kept as its own commit, f71e0b1a38b. **The user has not
  decided on it yet.**
- v1-sigterm, a96aa7894d8 and cee6fd48603. Auto-merged with bin-commands in exec/lib.rs and
  kernel/lib.rs. The atomic-wait rewrite lives inside `instrument`, so the Loader and the CLI's
  CachedLoader get it, and the cache key is the hash of the instrumented bytes, so no stale
  cached modules. The hooks replace instructions inside asyncify-guarded code, so they do not
  run during an unwind or rewind.
- userland-fixes, 4 commits without its tests bump. One DESIGN.md decision-row conflict: kept
  both rows. asyncify.rs auto-merged with train4's `__stack_pointer` fix.
- tests: 7795723, c894aa8, 6bd1837, 4982cf3 and 18c1440 cherry-picked onto b0861b8.
- libc2: 56931561 cherry-picked; README status row for fork/_Fork/vfork (62f9ce9f).

### Review fixes on top (coordinator scope addition, from v2-review.md)

- H6, cb7ba2abef0: a `Binfmt` trait in wasixv2-syscalls. The kernel asks it, after the `#!`
  handler and before `exec_commit`. The facade's `Abis` implements it as the new
  `Loader::probe` (by default wasm magic and version 1, failing with
  `LoadError::Format(errno)`) followed by `Abi::matches_image` (import section). `LoadError` is
  now an enum (`Format`/`Other`); the CLI loader maps its errors with `LoadError::other`.
  Images nothing serves now fail with ENOEXEC instead of 139: no ABI imports, wasix_64v1, a
  component, a truncated module. Tests are in api/tests/commands.rs. The new test fails with
  139 without the fix.
- M-P8, d511736653d: `kernel_execve` takes only the Exec action (`Work::take_exec`), and
  returns EINTR instead of panicking. Kernel test: an exit queued before the image starts
  survives.
- WP12/M-LIBC1: libc2 a838d039 deprecates the vfork function declaration (the macro does not
  warn; `(vfork)()` and function pointers do), deletes the stale comment and updates the
  README row. wasmer 5b5d17fe57f updates the DESIGN row and section (ENOSYS without asyncify).
- Extra, not in the review, 926bb24cc9f: the SIGTERM smoke of the v1 servers showed a host
  panic, "syscall from an unknown task". Two tasks are released at once on different host
  threads: `exit_files` collected the leader's table while the other release removed the
  leader's task, and then `closed()` looked the leader up again. Files now close by pid
  (`closed_by`). Deterministic kernel test. The panic was already in the v1-sigterm agent's own
  smoke logs.

### Validated

- Final tree (926bb24cc9f): 521 crate tests passed, 0 failed; clippy with the lint flags clean
  (lib/wasixv2 crates `--all-targets`, wasmer-cli, `--test run`); fmt clean. CLI `--test run`:
  51 passed, 0 failed, 13 ignored. `issue_3794_unable_to_mount_relative_paths` is fixed and 5
  new bin-commands tests pass; nothing that passed on bdcdb958bef fails.
- Suite, tests 6405893 with the libc2-train5 sysroots. run1 was before the review fixes; run2
  had H6, M-P8 and WP12 (libc2 a838d039, rebuilt sysroot). Both runs: 2349 passed, 1231
  expected failures, 0 unexpected failures, 0 unexpected passes, 454 ignored. Their failure sets
  are identical to the in-tree lists, so no bless commit is needed and flaky.txt and the
  hand-written reasons are unchanged. The exit fix (926bb24cc9f) came after run2 and has had no
  suite run yet.

| Suite | pass | expected failures | ignored | vs b0861b8 |
| ----- | ---- | ----------------- | ------- | ---------- |
| posix | 1328 | 465 | 83 | = |
| ltp | 464 | 738 | 370 | = |
| libc-test | 393 | 28 | 0 | = |
| own (suite/) | 165 | 0 | 1 | +5 new, all pass |

- Smoke run-all.sh (binary before the exit fix): sws-v2 28/28; v1 8/8 pass; python fails as
  expected.
- Extras (same binary): `wasmer run wasmer/bash -- -c 'ls /bin | wc -l; echo hi | cat; readlink
  /proc/$$/exe'` printed 85, hi, /usr/bin/readlink. That is correct: `bash -c` execs its last
  command in place. The v2 userland webc and out/bin-at-/bin both printed x, 4194304, 1. v1 sws
  and phpix `-S` stop on SIGTERM, but with the host panic above (exit 127; 127 is also the
  status the guest's handler passes to exit_group after abort).

### Remaining before landing

1. Rebuild release (gates on the final tree are done), run one full suite on 926bb24cc9f, and
   re-run the extras: SIGTERM must now exit with no panic. extras.sh has a fixed timing line.
2. Exec speed comparison was not done: exec-exit-with-threads before and after, base binary at
   logs/train5-land/bin/wasmer-base.
3. Land in order, fast-forward only: libc2 main (aaadefd0, 62f9ce9f, a838d039, plus a tests
   bump to the tests main commit without `wip:`), wasixv2-tests main (6405893), wasmer
   feat/wasixv2 (rewrite the `wip:` bump). Then fast-forward the shared libc2 `integration`
   checkout (its abi submodule is already at bcd7622) and rebuild its sysroots
   (`make wasixv2-sysroot wasixv2-sysroot-asyncify`; cmake is not on PATH, use
   /nix/store/jcvvpih1046akxcwh9hdiak5q24jqcf0-cmake-4.4.3/bin and run the two targets one
   after the other).
4. Delete /home/theduke/dev/github.com/wasmerio/wasmer-train4-base (not done yet).
5. The user still has to decide on the v1 0777 create-mode commit (f71e0b1a38b).
6. WP2's remainder (H10 exec cost, identity-keyed cache, run_file single compile, the CLI's
   check_command_atom now overlaps the kernel probe) belongs to another agent.

### Landing (2026-10-27, second machine)

Validated on a fresh toolchain (LLVM wasix-22.1.8 `3f3e05caa210`, wasixcc `8222197`, libc2 `train5`
`0006659a`, binaryen 129) with the final tree (`926bb24cc9f` included):

- `gates.sh release` EXIT 0.
- Two full unblessed suite runs, both 2349 pass, 1231 expected failures, 0 unexpected failures,
  0 unexpected passes, 454 ignored (Open POSIX 1328/465/83, LTP 464/738/370, libc-test 393/28/0,
  wasixv2 165/0/1). The `ef` diffs differ from the lists only in a temp path and one hand-written
  reason: no bless commit.
- Smoke: sws-v2 28/28; v1 8 pass, python expected-fail. Extras: v1 sws and `php -S` stop on SIGTERM
  (exit 127) with no "syscall from an unknown task" panic; `wasmer/bash` prints 85, `hi`,
  `/usr/bin/readlink`; the userland package and `out/bin` pass.
- Exec cost against `feat/wasixv2` `bdcdb958bef` (3 runs each, warm and cold):
  `exec-exit-with-threads` 0.7–1.2 s vs 6.9–9.6 s; `proc-` 1.0–1.6 s vs 3.6–4.3 s (the base fails
  one `proc-` test, a test train 5 added). The module cache makes exec about 10× cheaper.
- The first suite attempts failed with ENOSPC (each worktree `target/` is ~20 GB): keep about
  60 GB free before a full suite run.

Landed, fast-forward: tests `main` `b0861b8..6405893`; libc2 `main` `5d33ff2a..c26931b5`
(`0006659a` without `wip:`); wasmer `feat/wasixv2` `bdcdb958bef..f98fc48fa5c` (`0269525c9eb`
without `wip:`). `f71e0b1a38b` (wasix1 0777) kept, as decided.
