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
