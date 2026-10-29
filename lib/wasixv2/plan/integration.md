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

## 2026-10-27: train6 (WP3 futex + mm, WP5+6 net, WP9 signals) — LANDED

wasmer `wasixv2/train6-land` on train 5: WP3 10 commits (rebased `--onto`, without the v1-sigterm
commits; adds a fix for a host panic when a sibling is released during the `exit_group` futex walk,
and a non-hanging wasix1 `atomic_waits` test), WP5+6 15 commits (the split of `70c542fa87b`; tree
identical), WP9 8 commits (one per finding; tree identical to `8d686147337`), the tests bump, a fix
(`f5492741a92`: a vfork parent whose group exited does not return from vfork; the kernel passes the
pending exit in `VforkParent` on `VforkReturn`/`Exec` and the exec layer unwinds with it) and the
re-bless bump. Conflicts: only the two documented ones (`real_timer_expired`, DESIGN vfork).

- Gates: crate tests 580, clippy, fmt, release, CLI `--test run` 51/0/13.
- First suite (two runs, before the fix): `proc-vfork-parent-exit` failed 5/5 (the exec layer
  resumed the vfork parent before its queued exit). Fixed as above; 5/5 pass after.
- Re-bless: `ltp/syscalls/mremap05` (WP3, H2) and `ltp/syscalls/tgkill02` (WP9, M-P4) come off;
  hand-written reasons kept. No Open POSIX/LTP socket test newly passes from WP5+6.
- Final unblessed suite: 2364 pass, 1229 expected failures, 0 unexpected, 454 ignored. All 13 new
  tests pass.
- Smoke: sws-v2 28/28; v1 8 + python expected-fail; v1 servers stop on SIGTERM with no panic. Their
  exit status 127 is the guest's: v1 static-web-server has no SIGTERM handler on wasm, wasix-libc's
  default action calls `abort()`, which ends in `_Exit(127)`.

Landed, fast-forward: libc2 `main` `c26931b5..a524136a`; tests `main` `6405893..5af6416`; wasmer
`feat/wasixv2` `f98fc48fa5c..53813911d17`.

## 2026-10-28: WP1 (exec/de_thread) — LANDED

wasmer `wasixv2/wp1-exec-2`: H4, M-P2, H3, L-P2, M-P3, M-P1, M-P5 (rebased onto train 6; two
mechanical conflicts in `thread.rs` `hand_image`), a design fix `f261286befb` (under H3 a non-leader
`execve` hands the vfork-waiting leader an `Exec`, so `VforkParent` carries `resumes: bool` instead
of the exit status: a parent with an `Exit` or `Exec` queued is not resumed; Linux
`zap_other_threads` in `wait_for_vfork_done`), and the tests bump. H3 commits from the calling
thread after the siblings are gone (Linux `de_thread`), not from the leader's side as the review
suggested: the leader's side has no guest memory for the robust-list walk and `mm_release`.

- Gates: crate tests 591, clippy, fmt, release, CLI 51/0/13.
- Suite (two runs): 2369 pass, 1229 expected failures, 0 unexpected, 454 ignored; the 5 new tests
  pass (`exec-execve-from-handler`, `exec-execve-cloexec-threads`, `exec-execve-mask-from-thread`,
  `proc-execve-argv-bytes`, `proc-vfork-parent-exec`). No list change, no bless.
- Smoke as train 6.

Landed, fast-forward: libc2 `main` `a524136a..bb5d6d71`; tests `main` `5af6416..eac8874`; wasmer
`feat/wasixv2` `53813911d17..e595aea505f`.

## 2026-10-28: WP4 (fs/vfs) — LANDED

wasmer `wasixv2/wp4-fs-2`: 18 commits (H1 RESOLVE_IN_ROOT from `dfd` as Linux `path_init`; M-FS1..5;
L-FS1..10, L-FS12 in part) rebased onto WP1 without conflicts, and the tests bump (13 suite-test
commits). Gates: crate tests 605, clippy, fmt, release, CLI 51/0/13. Suite (two runs): 2380 pass,
1229 expected failures, 0 unexpected, 454 ignored; no upstream test newly passes, no bless. Smoke as
WP1.

Landed, fast-forward: libc2 `main` `bb5d6d71..0bc9eed3`; tests `main` `eac8874..7fd6227`; wasmer
`feat/wasixv2` `e595aea505f..9c147516805`.

## 2026-10-28: WP2 (rest) + WP7 (wasix1) — LANDED

wasmer `wasixv2/wp2-wp7-land`: L-FS11; H10/M-CLI3 (256-byte probe before the whole file, shared
package images, a cache keyed on identity or a raw-bytes hash, instrumentation on a miss only);
M-CLI2/L-CLI3 (`run_file` = `Kernel::exec` + the image loop, one compile in `wasmer run`); M-W5
(v1 command defaults keyed by the command file); L-CLI4 + docs; H11 (systemd-resolved upstreams for
v1 DNS); H5 (scratch window faults; v1 `memory.grow` stops below it via a `wasixv2-exec::memory-grow`
hook, not guest ABI); H12 (end the signal hold old wasix-libc leaks in `pthread_create`); M-W1; M-W4;
L-W5; L-W2; L-W3; docs for M-W2, L-W1, L-W4 (0777 kept), L-W6. 14 commits plus the tests bump.

- Gates: crate tests 620, clippy, fmt, release, CLI 51/0/13.
- Suite: two copy runs 2381/1229/0/454, identical; check run against the new lists 2380/1229/0/455.
  No test newly passes. `pthread_once/6-1` marked flaky: hangs 3/40 on the base and 4/40 on the
  branch (main thread stuck in `pthread_join` after every thread exited).
- Smoke: sws-v2 28/28; v1 8 + python expected-fail (python now exits 139, see PLAN follow-ups);
  extras as before; v1 `gethostbynamel` works on a systemd-resolved host. Exec bench: 4.9 s to
  0.40 s for 100 fork+exec of `/bin/true` from v2 bash.

Landed, fast-forward: libc2 `main` `0bc9eed3..f216f303`; tests `main` `7fd6227..d54b93c`; wasmer
`feat/wasixv2` `9c147516805..783b2cfcf2f`.

## 2026-10-28: WP10 (facade/CLI) — LANDED

wasmer `wasixv2/wp10-cli`: first-program load errors (`ExecError::Load`/`Instantiate`, exit 1),
M-CLI1 (Ctrl-C counts what the kernel discarded), M-CLI4 (default `HOME`/`PATH`, script directory
at `/run/wasmer/script`, `--use`), M-CLI5 (`Layer` API, packages mounted once), M-CLI6 (guest
threads on a multi-thread runtime, `Machine::shutdown()`, kill on drop), L-CLI1/2/3/5. 12 commits,
no tests-repo change. Gates: crate tests, clippy, fmt, release, CLI 59/12. Suite (two runs):
2380/1229/0/455, identical pass sets. `pthread_join/speculative/6-1` (expected failure) exits 132 or
139 depending on which of its two traps comes first, on every binary. Smoke: sws-v2 28/28; v1 8 +
python expected-fail (exit 1, "instantiation failed"); extras pass (bash prints `/bin/readlink`).

Landed, fast-forward: wasmer `feat/wasixv2` `783b2cfcf2f..11d0ae2781b`.

## 2026-10-28: WP8 (syscall hot-path performance) — LANDED

wasmer `wasixv2/wp8-perf-2`: pid cache per v1 instance, `AsyncFunctionEnvMut::try_read`/`try_write`
(lib/api, with sys tests), guest copies under a read lock without a boxed future (M-X2), v1 imports
filtered by module (M-W3). Gates green. Interleaved A/B against `11d0ae2781b` (7 runs, median): v1
getpid 449→426 ns, read 790→712, write 827→732; v2 read 585→557; v1 thread create+join 190→59 us;
v1 spawn 498→318 us. Suite (two runs): 2380/1229/0/455. Smoke as WP10.

Landed, fast-forward: wasmer `feat/wasixv2` `11d0ae2781b..9d04b0b7899`.

## 2026-10-28: ABI codegen (imports filtered by module, `take_image` → `Image`) — LANDED

wasixv2-abi `exec-imports`: generated `imports(store, env, module)` defines only the `wasixv2`
functions the module imports; `Control::take_image` returns `Option<Image>`. wasmer one commit
(bump, regeneration, `ExecImage` removed); libc2 a pointer-only bump (its generated C is unchanged;
both sysroots rebuilt and identical apart from musl's version string). Gates: crate tests 634, CLI
59/0/12. A/B against `9d04b0b7899`: v2 thread create+join 358→91 us, v2 spawn 620→259 us, v2 read
592→558 ns; v1 unchanged. Suite: run 1 2380/1229/0/455; run 2 broken by a mid-run swap of the
shared clang (420 compile failures, "Failed to run command clang"); rerun 2379/1229/1/455 with
`timer_getoverrun/2-2` flaky on every binary (9/20 vs 11/20 alone). Smoke as WP8.

Landed, fast-forward: abi `main` `bcd7622..832bb78`; libc2 `main` `f216f303..3bcd192c`; wasmer
`feat/wasixv2` `9d04b0b7899..dba44c5eba2`.

## 2026-10-28: merge train 7 — LANDED

wasmer `wasixv2/train7` (23 commits on `dba44c5eba2`): kernel-followups, init-vfork (+ a combined
vfork/ExitMm test), wp11-robust, low-guard, DESIGN `__linux__`, the lock sweep. tests `train7`:
runner-uid0-asyncify, kernel-followups, init-vfork, low-guard fixtures, bounded capture
(`9089eb0`), `WASIXV2_TESTS_WRAPPER` (`868b636`), the LTP heartbeat fix pointer (`5cd2035`), the
re-bless (`9408271`) and flakes (`c08bcd5`). libc2 `train7`: linux-uapi rebased + tests bump.

- Conflicts: ExitMm vs the removed ExecImage; `exit_notify` (vfork "gone" vs `left_mm`); Cargo.lock;
  the CLI spawn snapshot now prints "Child pid: 3" (init).
- OOM: suite #1 died four times. Causes: unbounded output capture of a spinning fork test's trace
  rerun (850 MB), and fork tests with 1000-2000 processes (8 MiB each, full copy). Fixed in the
  runner (bounded capture; per-test systemd scope, 10 GB, inside an 11 GB slice).
- 398 regressions before the LTP fix: 395 LTP heartbeat (`getppid() == 1` sees init in the fork's
  in-process mode), `mmap/24-1` (cap), `clock_getcpuclockid/1-2` (expected failure, pid 1's CPU
  clock). After: one (the clock test).
- Bless: 257 off (posix 152, ltp 99, libc-test 6), 194 on (posix 1, ltp 193). Flaky: timer_getoverrun/2-2,
  aio_suspend/1-1, fcntl14, fcntl14_64.
- Gates (after the sweep): crate tests 669, clippy, fmt, release, CLI 59/0/12. Suite #3:
  2781/1165/0/714. Smoke: sws-v2 28/28 (also the `__linux__` build), v1 8 + python expected-fail,
  extras as before; programs run as pid 2 with ppid 1.

Landed, fast-forward: libc2 `main` `3bcd192c..22a372c9`; tests `main` `d54b93c..c08bcd5`; wasmer
`feat/wasixv2` `dba44c5eba2..c4c59c64f70`.

## 2026-10-28: merge train 8 — LANDED

wasmer `wasixv2/train8` (37 commits on `c4c59c64f70`): kernel follow-ups 2 (faulting reads keep data,
FICLONE*, uname 7.2.0, splice/tee/vmsplice), job control 1-3 (stop/continue, console tty on
`/dev/hvc0` with raw mode and `--no-tty`, ptys and devpts), kernel follow-ups 3b (other processes'
CPU clocks, ExitMm at kill, vfork phase B with review fixes), sparse fork copy, shared anonymous
mappings, SysV segments and memfd//dev/shm files on host shared pages, futex keys on shared objects.
tests `train8`: their fixtures, LTP shell tests (`WASIXV2_TESTS_USERLAND`), 11 C++ fixtures, the LTP
pointer to `wasixv2-shared`, the re-bless. libc2 `train8`: libc++/libc++abi/libunwind in both
sysroots. wasixcc: `wasix++` for wasixv2.

- Conflicts: mostly train 7's lock sweep (applied to the branches' code); `Io.task` vs `read_to`
  unified as `OpenFile::io(task, interrupt)`; duplicate `TaskTable` helpers dropped.
- Gates: crate tests 805, clippy, fmt, release, CLI 59/0/12, `run_tty` 6/6.
- Suite #1: 187 newly pass (posix 41, ltp 146), 218 LTP entries now skip (shell tests, missing `ip`,
  `mount`, `ifconfig`), 11 newly run and fail (9 pty tests: the test runs as session leader, as on
  Linux; ldd01_sh; mlockall03), 1 regression that was always a failure (madvise10: the child's TFAIL
  now reaches the parent through the shared LTP IPC region).
- Bless: 408 off, 12 on with hand-written reasons. Suite #2: 3006/769/0/935.
- Smoke: sws-v2 28/28 (classic and `__linux__`), v1 8 + python expected-fail, C++ programs on both
  sysroots, `wasmer run bash` on a host pty 11/11 (bg, jobs, fg, ^Z, ^C, exit status), `--no-tty`.

Landed, fast-forward: libc2 `main` `22a372c9..2c3c3d4f`; tests `main` `c08bcd5..e08d138`; wasixcc
`feat/wasixv2` `8222197..d6373e5`; wasmer `feat/wasixv2` `c4c59c64f70..b75d9d49205`.

## 2026-10-28: merge train 9 — LANDED

abi `train9` (sock-extended-err, ns-ioctls, cpu-time codegen), wasmer `wasixv2/train9` (19 commits on
`b75d9d49205`: follow-ups 4-6, gap A UDP/errqueue, gap P namespace scaffolding, gap B CPU time, one
combined ABI bump), tests `train9` (fixtures, the LTP launcher, `upstream/ltp` -> `wasixv2-ipc`, the
re-bless, flakes), libc2 `train9` (abi pin), wasixcc `train9` (clone3 asyncify), plan (UDP smoke).

- Conflicts: mostly train 7's lock sweep; `SharedMappings` gained `wipe_on_fork` in the exec test
  Recorder; ns vs cpu-time in `process.rs`/procfs; the LTP launcher only on the non-shell path.
- Gates: crate tests 866, clippy, fmt, release, CLI 59/0/12, run_tty 6/6.
- Suite #1: 43 newly pass (madvise10, 9 pty tests, pty02, setpgid01, futex_wait02-04,
  futex_wake01-03, futex_cmp_requeue02, cve-2018-6927, times03, setitimer01, setrlimit06, fork/8-1,
  ioctl_ns02-04, unshare02, openat202, recv01, recvfrom01, execl01/le01/lp01/v01/ve01/ve06/vp01,
  cve-2021-4034, setpgid03, shmat01). No regressions.
- OOM: four libc-test OOM tests commit 4.2 GB each (also on feat): the kernel zeroed reused pages by
  writing; fixed on `wasixv2/vmfill-commit` (train 10); two of them flaky meanwhile.
- tgkill01: a wrong-thread SIGUSR1 caused by the LTP fork's in-process heartbeat (being fixed).
- Final check: 3053/725/0/0/940. Smoke: sws-v2 28/28 (classic, `__linux__`), v1 8 + python,
  UDP publish 6/6, C++ 9/9, pty bash 11/11, `--publish 15353:53/udp` with `--user 0`.

Landed, fast-forward: abi `main` `832bb78..f9958c7`; libc2 `main` `2c3c3d4f..449e092e`; tests `main`
`e08d138..89a2785`; wasixcc `feat/wasixv2` `d6373e5..7ab1ffb`; wasmer `feat/wasixv2`
`b75d9d49205..0bd23caafbb`; plan `50acb462dc9..043df85f084`.

## 2026-10-29: merge train 10

abi `train10` (statmount, 340b9f3), wasmer `wasixv2/train10` (34 commits on `0bd23caafbb`: gap D UTS/IPC/net
namespaces, gap C mounts and mounts phase 2 (mount namespaces, mount syscalls, new mount API,
statmount/listmount), one ABI bump, fork-cow, vmfill, follow-ups 7 and 8 (incl. kernel copies that fault
on unmapped and PROT_NONE pages, CLONE_PIDFD), the console ^C test fix, the tgkill race fix
(complete_signal targeting), the tests bump), tests `train10` (fixtures, mounts bench, the runner's
fork detection for stackless clone, `upstream/ltp` -> `wasixv2-heartbeat` 4f386da55, flaky.txt
-tgkill01/malloc-brk-fail/setenv-oom, the re-bless, ptimer-abstime), libc2 `train10` (abi pin, tests),
plan (userland-3).

- Conflicts: the branches predate train 9's lock sweep (resolve.py); ns.rs: gap C and gap D merged onto
  one per-kernel inode allocator (`Inums`) and `make(kernel, kind, fs)`; vmfill vs fork-cow:
  `zero_pages` takes the layout and `Base::clear`s inside the frozen image, drops past it (new exec
  test); PROT_NONE vs vmfill: `zero_pages` is a kernel fill (default via `force_write`).
- Gates: crate tests 974, clippy, fmt, release, CLI 59/0/12, run_tty 6/6.
- Suite #1: 77 off the lists (about 40 EFAULT tests, mount/ns tests, containers, mlock, close_range02),
  111 newly run and passing (99 fs_bind, tar01, cpio01, gzip01, unzip01, statmount08, listmount03,
  tgkill01, malloc-brk-fail, setenv-oom, 13 fixtures), 8 newly run and failing (6 fs_bind umount EBUSY
  timeouts, unshare01_sh, listmount04), 6 regressions approved as expected failures: 5 LTP tests use
  guarded buffers the LTP fork's in-process mode freed (EFAULT now), posix mlockall/8-1
  (RLIMIT_MEMLOCK counts the whole linear memory).
- Suite #2: 3233/662/1/0/835; the one, ptimer-abstime, a load flake of the test (also on train 9's
  binary), fixed in the test. Filtered recheck 0 unexpected.
- Smoke: sws-v2 28/28 (classic, `__linux__`), v1 8 + python expected-fail, UDP 6/6, extras (classic,
  userland-3), C++ 9/9, pty bash 11/11; fork-cow 0.13 MiB host RSS per child (32 MiB data, 32
  children); vmfill tests 42-44 MB (were 4.2 GB); tgkill01 `-i 20` stress 0/320.
- Follow-ups: LTP fork keeps guarded buffers in the in-process mode; RLIMIT_MEMLOCK accounting of the
  stack; fs_bind umount EBUSY on self-propagated mounts; shared propagation back from a new mount
  namespace; listmount04 error order and flags.

Land in order: abi `main` `f9958c7..340b9f3`; libc2 `main` `449e092e..1e68cf76`; tests `main`
`89a2785..eb945b2`; wasmer `feat/wasixv2` `0bd23caafbb..8e19e34e15e`; plan.

## 2026-10-29: merge train 12

wasmer `wasixv2/train12` (42 commits on `55f0180df02`: follow-ups 11, net-config, user namespaces,
follow-ups 12 with its ABI bump, net follow-ups, three integration commits, the tests bump), tests
`train12` (fixtures, LTP with libcap and the user-namespace, dnotify and taint kconfigs, the network
data files, network tests as root, a procdev-proc-maps fix, the re-bless), libc2 `train12` (abi pin
`f410936`, tests), abi `pidfd-info` `f410936`, plan (userland-4: iproute2, net-tools, iputils).

- Conflicts: the branches predate train 11 (pid namespaces, SIGIO, `PidRef`) and each other. procfs's
  static table and `Write` (nr_open, devconf, dnotify/leases, `/proc/sys/user`, tainted, vm and pipe
  sysctls); `fsopen` contexts (pid namespace, network namespace, user namespace, IPC namespace) and
  the filesystem types (proc, sysfs, mqueue); SysV `to_abi` with both the pid and the user namespace;
  net-config's interface ioctls moved to `net/if_ioctl.rs`; `task.ns` became an `Option`.
- Integration: one `/proc/sys/net` (net-config's `sysnet.rs`), its IPv4 `conf` values train 11's
  `devinet::Devconf`, and follow-ups 12's `net_ctl_permissions` for the whole tree; net-config's
  capability checks over the network namespace's owner (`dev_ioctl`, `packet_create`,
  `rtnl_link_get_net_capable`), sysfs `FS_USERNS_MOUNT(_RESTRICTED)`; raw sockets and
  `ping_group_range` (kernel gids) under user namespaces; the VFS's initial user namespace is the
  kernel's (`copy_mnt_ns` made root's copies slaves: fs_bind_cloneNS regressions).
- Gates: crate tests 1125, clippy, fmt, release, CLI 59/0/12, run_tty 6/6.
- Suite #1 (vs train 11's #3): 13 off the lists (leases, vm sysctls, ioctl_ns06, socket01,
  sched_setaffinity01, cve-2018-5803), 89 newly registered, 7 regressions fixed (5 fs_bind_cloneNS,
  procdev-proc-maps: maps shows mallocng's PROT_NONE guard, a test fix; cve-2014-0196 flaky).
- Suite #3 (check): 3385/654/0/0/811; posix 1521/270/85, ltp 1187/362/725, libc-test 399/22/0,
  own 279/0/1. sendfile01/601, ping01/601, netstat01 pass as root.
- Smoke: sws-v2 28/28 (classic, `__linux__`), v1 8 + python expected-fail, UDP 6/6, extras,
  userland-linux/-3/-4, C++ 9/9, pty bash 11/11; by hand pid-ns container, `unshare -r`/`-rn`, ip
  (link/addr/route/rule/dummy), ping 127.0.0.1 and ::1, unprivileged ping with `ping_group_range`,
  a veth pair between namespaces pinging both ways.
- Follow-ups: `/proc/sys/user/max_*_namespaces` and `pid_max` show 0666 where Linux has 0644 with
  `set_permissions`/`pid_table_root_permissions` (16 LTP tests); mc_commo/mc_member time out as
  root; userns06 resource copy; wait403 (ESRCH for INT_MIN); IPV6_ADDRFORM, IPV6_RTHDR,
  PACKET_VERSION; devconf ownership (`net_ctl_set_ownership`); max_map_count's O(n) area scan;
  fcntl35's privileged half.

## 2026-10-29: merge train 14

Light landing protocol (one suite run, regressions only). abi `train14` `d0187cb` (P1's codegen
`Layer<S>` and `Syscall::nr/args/ret_from_raw` and its seccomp, Landlock, keyctl and LSM types; P4's
AIO, io_uring and rseq types; P5's file handles, inode flags, ACLs, quotas and fanotify types; picks
on `e7bb953`), wasmer `wasixv2/train14` (22 commits on `908423d1510`: one ABI bump, P1 sandbox 6, P4
async I/O 6, P5 filesystem 6, P6 time 1, one integration fix, the tests bump), tests `train14` (the
re-bless), libc2 `train14` (abi pin with the asserts regenerated, tests).

- Dropped: P1's cherry-pick of netfu2's BPF and its compile fix (feat has both), every branch's own
  ABI bump, P4's base (fu13, in feat under other hashes).
- Conflicts: procfs's static table and modes (aio-nr/aio-max-nr, `/proc/keys`, dnotify and leases;
  aio-max-nr 0644 with train 13's sysctl modes); io_uring's by-file socket calls vs the packet
  sockets; `finish_open` (P5's `OpenMode`, permission events and `dentry_open_nonotify` around
  Landlock's `security_file_open` and train 13's `F_CREATED`); fanotify's read path in P4's
  `read_into_iovecs_flags`; P6's `Timekeeper` with the DNS modes in the api.
- Integration: one `f_op->show_fdinfo` hook (P4's `show_fdinfo() -> String`; P5's `fdinfo()` went);
  procfs's `ProcNode::permission` takes P5's ACL; futex2, io_uring_enter's `ABS_TIMER` and io_uring's
  absolute timeouts go through P6's time namespaces (`timens_ktime_to_host`).
- Gates: crate tests 1287, clippy, fmt, release, CLI 59/0/12, run_tty 6/6.
- Suite (copy, vs train 13's): 3471/595/15/19/761 (posix, libc-test, own unchanged). 19 LTP tests off
  the lists (file handles, clock_settime/settimeofday/stime/adjtimex/clock_adjtime01/leapsec01/
  cve-2018-11508, alarm timers, setxattr03); 12 newly run and listed (keyctl08/17/19/22,
  request_key04/06, cve-2016-9604, cve-2017-17807, fanotify02/04/11, landlock02); 3 regressions,
  blessed with their causes and reported: keyctl07 and cve-2017-12192 (request_key constructs no
  negative key; they passed while keyctl was ENOSYS), proc-vfork-sibling-exec (a host panic: the
  dead vfork leader's deferred `exit_mm` drops the namespaces of the leader task the thread's new
  image runs as, and `clock_gettime` now reads the time namespace). 245 LTP AIO tests are TCONF
  "requires libaio": the LTP build has no libaio, so P4's AIO is not yet covered by the suite.
- Smoke: as train 13 (sws-v2 28/28 classic and `__linux__`, v1 8 + python expected-fail, UDP 6/6,
  extras, userland-linux/-3/-4, C++ 9/9, pty bash 11/11, `--no-tty`, the train 13 hand checks); by
  hand a seccomp ERRNO filter (and KILL_PROCESS in a child), a Landlock ruleset denying a write,
  keyctl add/read/request/update/revoke and `/proc/keys`, io_uring NOP + READ and an armed pipe read,
  fanotify on a directory, clock_settime as root (the guest moves a day, the host does not; EPERM
  unprivileged), `unshare -T --monotonic --boottime` (timens_offsets, uptime; EACCES once entered).
- Follow-ups: fanotify events report pid 0 in CLI runs (`current_task()` unset; set it at the
  syscall entry layer instead of the executor); negative keys and key construction; the vfork-leader
  exec namespace drop; `/proc/sys/kernel/keys/*`; Landlock's `security_file_open` is in `Vfs::open`,
  not `finish_open` (fanotify's event descriptors skip it); io_uring REALTIME absolute timeouts do not
  re-arm on clock_settime; libaio in the LTP build.

Land in order: abi `main` `e7bb953..d0187cb`; libc2 `main` `b69aa1d3..5e03a9fe`; tests `main`
`5e21625..cabce84`; wasmer `feat/wasixv2` `908423d1510..f9c8b36bad6`; plan.

## 2026-10-29: merge train 15

GO under the light landing protocol: one full suite, regressions checked against train 14,
new outcomes blessed in bulk, then a targeted rerun of the three regressing scheduler tests.
ABI `train15` `ce81dcf` (5 picks on `d0187cb`), wasmer `wasixv2/train15` `f850deddc36`
(27 commits on `f9c8b36bad6`), libc2 `train15` `6b478da6` (ABI pin/asserts and tests pin),
tests `train15` `fdd63ba` (SCTP fixture, runner config fix, re-bless).

- Features: P2 ptrace/Yama, foreign memory access, process_vm_readv/writev, proc memory and
  introspection, remap_file_pages; P3/P3b cgroup v2, pids/freezer/notifications, CPU and I/O
  controls, printk/syslog/kmsg, accounting, core dumps and more proc/sysctl files; P7/P7b vsock,
  SOCK_DIAG, raw protocols, Unix OOB/peek/autobind, IPv6 sticky headers, PACKET_VERSION and SCTP.
  Individual branch ABI bumps were dropped in favor of one shared pin.
- Conflicts/seams: seccomp keeps both user notification and tracer stops; read accounting wraps
  the flags/fanotify path; ptrace initialization, keyring reset, time namespaces and cgroups all
  survive process/exec merges; freezer waits retain ptrace stops; securityfs and cgroup mounts
  coexist. Corrected positional procfs/sysfs parents, adapted non-Copy credentials and socket
  handles, and kept core helpers on the syscall-entry stack. Yama appears in the active LSMs.
- Small runtime fix: the syscall-exit hook checks liveness before looking up a tracer link.
  An unparented tracer can already be reaped by exit_group; Linux `kernel/exit.c:do_exit` never
  returns through syscall-exit tracing. A new layer-level unit test reproduces the host panic
  before the fix (`9a3e24bfb42`); the tracer hand check exits cleanly afterward.
- Gates: full crate tests 1411/0, followed by kernel 818/0 including the new regression (1412
  unique crate tests covered); runner 13/0; clippy with Makefile lint flags, fmt, release, CLI
  59/0/12, tty 6/0. Both libc2 sysroots and run.sh checks pass.
- Full suite raw: 3514 passed, 589 expected failures, 12 unexpected failures, 11 unexpected
  passes, 736 ignored. Only three regressions: sched_rr_get_interval01/02/03 could not find
  kernel configuration after the new cgroup v2 mount made LTP's setup helper inspect it.
  The runner now supplies its existing KCONFIG_PATH to every LTP invocation, including those
  without needs_kconfigs metadata (`00d56c5`). Its unit test fails before the fix; the targeted
  rerun passes all three. No remaining regressions or pass-to-ignored changes. Nine other new
  failures did not pass the baseline and were blessed without further triage. All 18 handwritten
  failure causes were preserved.
- Final outcomes, including the targeted correction (pass / xfail / ignored): own 289/1/1,
  libc-test 399/22/0, POSIX 1538/247/91, LTP 1302/328/644; total **3528/598/736**. No second full
  suite was run.
- Smoke: sws-v2 classic and Linux 28/28 each, v1 8 plus the known Python expected failure,
  UDP 6/6, extras, userland-linux/-3/-4, C++ 9/9, pty bash 11/11 and no-tty; train 14 hand checks
  have the same exit sequence. New hand checks pass: ptrace tracer (5 syscall stops and clean
  exit), cgroup pids.max EAGAIN/freeze/thaw, real util-linux dmesg and dmesg -S, SIGABRT core
  status plus ELF file, vsock stream and seqpacket loopback, ss -tanp with pid/fd, SCTP fixture.
- Carry the train 14 follow-ups and package design limitations; no additional refinement was
  attempted. Shared libc2 is fast-forwarded to `6b478da6`; both shared sysroots rebuilt and run.sh checks passed.

Land in order: ABI `main` `d0187cb..ce81dcf`; libc2 `main` `5e03a9fe..6b478da6`; tests `main`
`cabce84..fdd63ba`; wasmer `feat/wasixv2` `f9c8b36bad6..f850deddc36`; plan. Only the train branches
and `wasixv2/plan` are pushed by the integration agent; the orchestrator lands the mains.

## 2026-10-29: merge train 16 — NO-GO

Do not land this train until `ltp/syscalls/clock_gettime01` is resolved. Integration, gates,
one full suite, regression-only follow-up, smoke and shared sysroots are complete. The remaining
regression is deliberately unblessed. The orchestrator owns remote main/feat landing.

Candidates: ABI `train16` **47561c7** on `ce81dcf`; libc2 `train16` **c39b9060** on `6b478da6`;
tests `train16` **a3f1f7d** on `fdd63ba`; wasmer `wasixv2/train16` **8f08d480859** on
`f850deddc36` (40 commits). Both consumers pin ABI 47561c7 and the same tests head.

- Inputs: fix14's six fixes, P2b ptrace/lifecycle/syscall editing/introspection, P4b io_uring
  retention/provided rings/resources/opcodes, P5b binfmt handovers/quotas/fanotify/overlay/handles.
  Package ancestors already landed under other hashes were excluded using cherry/patch and
  package-log audits. WIP changes were folded into their conventional completion commits;
  individual ABI/test pins, P2b's P1 merge and redundant Yama change were omitted. Five ABI
  additions form one chain; Wasmer regenerated once, libc2 assertions match its golden file.
- Integration retains task namespaces, syscall context, per-thread I/O accounting and cgroups
  across non-leader exec; syscall entry retains current-task scope and train15's exit liveness
  guard. By-file descriptor destinations share existing open/socket checks, including Landlock,
  net capabilities, SCTP, packet and vsock. Key sysctls use parent 124 in the combined proc table.
- Small runtime fix `c5c49e45e04`: queued VFORK_DONE keeps a PidRef until delivery. Linux
  `kernel/fork.c:kernel_clone` holds get_task_pid through wait_for_vfork_done/ptrace_event_pid.
  The new test reports PID 0 instead of 3 before the fix, including child autoreap; the hand
  tracer now gets VFORK_DONE and signal EXIT once each and exits cleanly.
- Gates: all 23 crate test invocations passed (1487 tests initially), then kernel 889 after
  the PID fix and 890 after the key fix (1489 unique crate tests covered). Makefile-flags
  all-target clippy, fmt and release passed; final CLI 59 passed / 12 ignored, tty 6 passed.
- Exactly ONE full suite, CPUs 0-3, runtime source c5c49e45e04: raw **3526 passed / 582 expected
  failures / 2 unexpected failures / 15 unexpected passes / 737 ignored**. No new registrations,
  no pass-to-ignored regressions, no non-regression new failures. No new-pass triage or second
  full suite. Two targeted regressions were rerun unblessed.
- Fixed regression `request_key02` in `303055f1026`: an unrelated expired nested keyring must
  not replace a searchable miss with EKEYEXPIRED. Linux `security/keys/keyring.c` separates
  search_nested_keyrings traversal from keyring_search_iterator candidate validation. Its new
  unit test fails before the fix; it also covers live contents of an expired ring and explicit
  destination validation. Targeted request_key02 passes afterward.
- Remaining regression: `clock_gettime01` repeats exit 137 from LTP's own 30-second watchdog.
  Setup spins on `/proc/self/stat` until utime becomes nonzero, before any clock assertions.
  Targeted trace shows the open/read/lseek/close loop; no host panic was observed. The executor
  CpuSample estimates every syscall's system cost from each 256th call; periodic workloads can
  alias an expensive sample and leave user time at zero (inference from code and this trace).
  Proposed follow-up: design user/system attribution using observed guest/kernel boundaries
  or independent mode sampling, following Linux `kernel/sched/cputime.c:account_process_tick`,
  with a deterministic mixed-cost syscall-loop regression. No sampling constant tweak or
  kernel/exec contract change was made. Logs and archived repro: `~/wasixv2-orch/logs/train16/`.
- Final observed outcomes after the targeted key correction (pass / xfail / unexpected fail /
  ignored): own **290/0/0/1**, libc-test **399/22/0/0**, POSIX **1538/247/0/91**, LTP
  **1315/313/1/645**; total **3542/582/1/737**. `ef-corrected` records actual outcomes;
  `ef-final` expectations intentionally leave clock_gettime01 unlisted. All 15 remaining
  handwritten causes were preserved; the three repaired handwritten cases and fanotify11
  were removed as requested.
- Final smoke: sws classic/Linux 28/28 each; v1 eight plus known Python expected failure;
  UDP 6/6; extras and all userlands; C++ 9/9; pty bash 11/11 and no-tty. Exit sequences match
  train15. All 13 feature commands pass: prior seven plus VFORK_DONE/signal EXIT, provided-buffer
  ring RECV and SPLICE, binfmt O interpreter with AT_EXECFD script contents, fanotify mount
  attach/detach IDs, tmpfs EDQUOT and request_key negative caching. Final runtime 303055f1026
  and full-suite runtime c5c49e45e04 are archived outside repositories.
- Shared libc2 local main fast-forwarded 6b478da6..c39b9060; both shared sysroots rebuilt with
  MAKEFLAGS= and OMP_NUM_THREADS=3, nice19/CPUs4-7; both run.sh checks EXIT 0. No remote main pushed.
- Carry package limitations: exact mmap fault quota accounting/SIGBUS needs an agreed kernel/exec
  contract; host handle decode requires host CAP_DAC_READ_SEARCH and native bodies over 116 bytes
  remain unsupported; io_uring SQPOLL, IOPOLL, NO_MMAP and REGISTERED_FD_ONLY remain unsupported.

After the clock regression is fixed and validated, landing order is ABI -> libc2 -> tests ->
wasmer -> plan. Current candidate ranges: ABI `ce81dcf..47561c7`; libc2 `6b478da6..c39b9060`;
tests `fdd63ba..a3f1f7d`; wasmer `f850deddc36..8f08d480859`. Only train16 branches and the
fast-forward `wasixv2/plan` are pushed by this agent. **NO-GO: hold main/feat landing.**

## 2026-10-29: merge train 17 — NO-GO

Hold main/feat landing: `ltp/cve/cve-2015-7550` timed out in the sole full run and
in its canonical 60-second targeted rerun. Integration, gates, one full suite,
regression follow-up, smoke, shared sysroots and train branch publication are
complete. The remaining regression stays unlisted. The orchestrator owns mains.

Candidates: ABI `train17` **3c21183** on `47561c7`; tests `train17` **aace62f** on
`c777afe`; libc2 `train17` **139386dc** on `c39b9060`; wasmer `wasixv2/train17`
**599db214dbf** on `a687dcd8a06` (47 commits). Both consumers pin the same ABI
and tests commits. Baseline is the orchestrator-landed train16 with
clock_gettime01 explicitly listed: **3542 pass / 583 xfail / 737 ignored**.

- Inputs: Q2 kernel miscellaneous interfaces (prctl, stable namespace IDs,
  proc/security/core notes, accounting, Unix urgent signals); Q3 io_uring
  NO_MMAP/registered-only rings, approved async task work, remaining opcodes,
  packet pipes and AIO VM_DONTCOPY; Q4 mapped tmpfs accounting, executable write
  denial, binfmt restrictions and notification pipes; Q5 inbound vsock backend,
  SCTP, netlink and NTP discipline, with v1 clock offsets in the compat layer.
  Applied Q4's KEYCTL_WATCH_KEY patch with its tests as a separate commit.
- Selected only package-owned commits. Excluded replayed P4b/P5b/early train15
  ancestors, WIP and per-package consumer pins. Five ABI additions form one
  fast-forward chain; Rust consumers regenerated once and libc2 assertions
  match the ABI golden. Layered forwards Control::run_task_work completely.
  All 134 procfs STATIC paths were audited (keys 124, seccomp 130); exec keeps
  train16 lifecycle/security behavior, auxv and per-mm/per-task prctl state.
- Integration correction `6143fec42a2`: reject notification pipes through
  io_uring's shared splice_files path as well as ordinary splice. Linux
  fs/pipe.c:get_pipe_info(for_splice=true), io_uring/splice.c:io_splice.
  The by-file unit regression reports EAGAIN instead of EINVAL before the fix.
- Host panic fixed in `ab864d40e67`: a parent can reap after take_pending
  selects an exit action, before exit_signals looks up its process. Use the
  existing optional Process snapshot; Linux kernel/exit.c:do_exit runs this
  cleanup before exit_notify/release_task. Deterministic unit red proof matches
  the vsock smoke panic. Final 30 repeated vsock runs/60 loopbacks have no panic.
- Gates: **1569 unique crate tests** covered, final kernel 954 and exec 51;
  Makefile-flags all-target clippy, fmt, release and final CLI **59 pass /
  12 ignored**, tty **6 pass**. Three integration fixtures were corrected for
  executable writer exclusion, a live exit observer and mapped ring ownership;
  their prior failing gates and focused reruns are retained in the train log.
- Exactly ONE full suite, CPUs 0-3, runtime `ab864d40e67`, 3138 seconds. Raw
  **3546 pass / 582 expected fail / 6 unexpected fail / 1 unexpected pass /
  727 ignored**. No registration changes or pass-to-ignored regression. Four
  other new failures did not pass baseline and were blessed without triage;
  no new-pass triage. All **16 handwritten causes** were preserved.
- Two regressions: fanotify11 reported the wrong event PID; cve-2015-7550 timed
  out. fanotify11 passed alone, but its deterministic exit-before-read unit
  exposed PID 0 instead of 2. Fixed in `c02326ab67a`: queued events retain the
  existing PidRef and serialize its namespace number after exit, matching
  fs/notify/fanotify/fanotify.c:fanotify_alloc_event's get_pid/put_pid lifetime.
  Kernel/clippy/fmt/release/CLI, targeted fanotify11 and final smoke pass.
- Remaining CVE: keyctl02 declares runtime 60 and LTP reports a 90-second overall
  timeout; the runner allows 60 seconds including startup/teardown. Both the
  full run and canonical targeted run timed out. A90-second diagnostic on the
  original suite binary passed in 24 seconds: this shows timing/load sensitivity,
  not proof that90 seconds is necessary or that the kernel hangs. Proposed
  follow-up: reproduce under a controlled 60-second budget with three jobs and define a
  metadata-aware runtime budget with headroom, using LTP's documented `-I`
  override (lib/tst_test.c:parse_opts), with runner unit coverage. No timeout
  constant tweak or suppression was made. Keep this regression unblessed.
- Final observed outcomes after the targeted fanotify correction (pass / xfail /
  unexpected fail / ignored): own **290/0/0/1**, libc **399/22/0/0**, POSIX
  **1538/247/0/91**, LTP **1321/317/1/635**; total **3548/586/1/727**.
  ef-corrected retains actual failures; ef-final intentionally omits the CVE.
- Final smoke: sws classic/Linux 28/28 each, v1 eight plus known Python failure,
  UDP 6/6, extras and userlands, C++ 9/9, pty 11/11 and no-tty. Ordinary/hand exit
  sequences match train16. All 20 feature commands pass, including MDWE/AUXV,
  listns 8 IDs, core NT_FILE/NT_AUXV readback, NO_MMAP task work completing on an
  unrelated syscall return, ETXTBSY and exit release, key watch records, nlctrl
  multipart dump, and adjtimex offset 1000->0us. Inbound host-to-guest vsock is
  skipped because the CLI exposes no VsockConfig/UnixPrefix flag; the inbound
  Unix bridge host-backend unit test passes. No new host panic remains observed.
- Shared libc2 local main fast-forwarded to 139386dc, matching ABI/tests pins;
  both shared sysroots and run.sh checks pass, MAKEFLAGS=, OMP_NUM_THREADS=3,
  nice 19/CPUs 4-7. No remote main pushed. Only own targets were removed.
  Logs, scripts, source metadata and both suite/final binaries are retained at
  `~/wasixv2-orch/logs/train17/`.

After the CVE timeout is resolved, land in dependency order: ABI `47561c7..3c21183`
-> tests `c777afe..aace62f` -> libc2 `c39b9060..139386dc` -> wasmer
`a687dcd8a06..599db214dbf` -> plan. Only train17 branches and the fast-forward
`wasixv2/plan` are pushed by this agent. **NO-GO: hold main/feat landing.**
