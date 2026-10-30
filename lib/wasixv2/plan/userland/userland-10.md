# Userland 10: upstream test suite validation

Runtime `wasixv2/v12suites` starts at `b6ba99822a3` (integration
`6a8410a5a45` plus the ptrace/SIGKILL regression fix). Tests `dff5da8`,
libc2 `6bc05b3b`, ABI `07cd32e`. Plan branch `wasixv2/plan-userland10`
starts at `01a0145d950`. All source builds and raw evidence are outside git,
under `~/wasixv2-tools/userland-10` and `~/wasixv2-orch/logs/v12suites`.

Use `BINARYEN=~/wasixv2-tools/binaryen`, source `../env.sh`, set
`WASIXCC_SYSROOT_PREFIX=~/wasixv2-tools/libc2-v11progs/build-wasixv2`
(the exact requested libc2 base, with both static sysroots built), and set
`CARGO_INCREMENTAL=0`. Each long job runs in a separate systemd user unit,
under `nice -n 19 taskset -c 4-7`, jobs/compiler threads 3; jobs are serialized.
Build the release CLI with `cargo build -j3 --locked -p wasmer-cli --release
--features cranelift`. No full WASIX or workspace suite is run.

Build source provenance follows userland-6 and userland-9: copy coreutils 9.12
and Bash 5.3.20 source trees into `userland-10/coreutils-src` and `bash-src`;
keep originals unchanged. Source archives and official Bash patches are pinned
by those recipes. Copy the static base utilities to `userland-10/bin`, and
source-built make/miniperl from userland-9; link `perl` to `miniperl`.
`bash userland-10.sh coreutils` and `bash userland-10.sh bash` configure new
out-of-tree builds against the exact-base asyncify sysroot.

Coreutils uses unmodified Automake `make check TESTS=...` in explicit filtered
batches, excluding the upstream root-only set and interleaving utility families.
Host-built `all-am` and maintainer/documentation-only `check-local` are marked
already built, and PROGRAMS= prevents re-traversing their host build dependencies; the upstream test driver, environment and expected comparisons
run in the guest. Each shell test has a 60-second bound plus a five-second kill deadline; --test/--label/--timeout allow isolated longer confirmations without replacing baseline results. The driver preserves
logs, counts `.trs` results and fails on host panics. A 90-minute suite budget
leaves not-attempted tests visibly distinct from upstream skips.

```sh
python3 tests/suites-10.py /absolute/release/wasmer coreutils --limit 12
# Omit --limit for the bounded broad non-root shell selection.
```

SQLite full source 3.49.2:
`https://www.sqlite.org/2025/sqlite-src-3490200.zip`, SHA256
`c3101978244669a43bc09f44fa21e47a4e25cdf440f1829e9eff176b9a477862`.
Tcl 8.6.16:
`https://prdownloads.sourceforge.net/tcl/tcl8.6.16-src.tar.gz`, SHA256
`91cb8fa61771c63c262efb553059b7c7ad6757afa5857af6265e4b0bdc2a14a5`.
Static Tcl builds with upstream `--disable-shared --disable-load` options.
Use `userland-10.sh tcl-nothreads`, then `TCL_CONFIG_PREFIX=$USERLAND10/prefix-tcl-nothreads
bash userland-10.sh sqlite` for the supported non-threaded Tcl profile. SQLite
itself remains THREADSAFE=1. Host Tcl generates SQLite sources; the target
tclConfig.sh supplies testfixture headers and static libraries. Static zlib from
the exact-base userland-9 prefix supplies testfixture's compression dependencies.
No linking, closures, libffi or reflection source changes were made.

Bash uses `make tests TESTSCRIPT=/suite/bash-batch-10.sh` with an explicit list
of `run-*` test groups (aggregate/duplicate scripts excluded). Helpers
recho/zecho/printenv/xcase are target-compiled, while build generators stay native.
Each group retains stdout/stderr, has a 90-second bound, and any expected-output
diff fails even if a later comparison would hide it. `run-shell-suite-10.py`
records the selected list and each result. Default informational warnings go to
stderr and are retained. Bash and Tcl builds run only after the preceding suite.

Initial corrected coreutils pilot: 7 PASS / 5 SKIP / 0 FAIL, no panic. Broad
non-root shell selection completed 614 tests: 488 PASS / 121 SKIP / 5 FAIL on the baseline. Two failures exposed symlink timestamp updates silently skipped in the host driver; three tail/strace failures exposed SIGTERM lost when SIGCONT queued a ptrace job-control trap. Both have failing-first Rust regressions and Linux source citations, followed by passing targeted crate tests, exact-flags clippy and formatting. All five fail-before cases pass on the fixed CLI; final combined counts are **493 PASS / 121 SKIP / 0 FAIL**. Isolated traced/untraced tail/sleep controls return the same status as native Linux. The initial timeout-budget suspicion was superseded by the confirmed ptrace signal-loss regression. Ancillary
programs supplied after the pilot make its first broad batch 8 PASS / 4 SKIP.
Two initial harness setup attempts executed zero tests (list parsing and
absolute utility paths); they are not included in counts.

Every kernel/libc correction has a failing-first sibling regression and Linux
source/function citation. Machine handoff and detailed chronology are in
`~/wasixv2-orch/logs/v12suites/STATE.md`.


Runtime fixes pushed by the AI agent with the required attribution:
`87cba5c4b5c` (host symlink times), `3d7fcca51fe` (resumed ptrace signal ordering),
`2a8d8408cde` (one timestamp per memfs attribute update).
Linux references: `fs/utimes.c:do_utimes_path/vfs_utimes` and
`kernel/signal.c:get_signal/ptrace_signal`. No ABI or kernel/exec contract change.
Host driver gates: 15 tests. Kernel gates: 33 ptrace +34 signal tests. Memfs
gates: 28 tests, with Linux `fs/attr.c:notify_change` as the reference. All three
touched crates pass exact Makefile-flags clippy and workspace formatting checks.
No full WASIX suite was run.


Bash cross configuration needs an actual target fork/wait probe:
`tests/probes/wait-status-10.c` prints the WEXITSTATUS offset (8 on the tested
Linux ABI). `userland-10.sh bash` runs it through `WASIXV2_PROBE_CLI` and passes
its measured result to configure. The upstream cross fallback 0 corrupts
lastpipe's synthetic process statuses. This is a build configuration correction,
not a kernel change. Use the source-built `/bin/dash` explicitly for POSIX wrapper
scripts; Bash POSIX mode exits on the suite wrapper's `unset` of readonly UID.
For tty tests, `run-shell-suite-10.py CLI --pty --label bash-pty` supplies an
isolated host PTY as the CLI's ordinary guest console, preserving expected files.
Initial no-tty/raw baseline was 75 PASS / 11 FAIL across 86 groups.

Tcl's first static link selected compatibility memcmp/strstr/strtoul from
unknown cross-run results and failed duplicate strtoul. The typed runtime probe
`tests/probes/tcl-compat-10.c` checks the same upstream conditions on the actual
target. Only after it passes does the recipe cache the successful results.
Core `binaries libraries` and corresponding install targets avoid bundled shared
extension packages. Static Tcl version/file/exec smoke checks pass.

## Completed suite results

Counts describe upstream assertions or groups as indicated, never source-build
success. Focused confirmations replace corresponding baseline failures without
double counting. Setup failures and partial diagnostic runs are retained outside
git and excluded from the final totals.

| Suite | Unit | PASS | FAIL | SKIP / untested |
| --- | --- | ---: | ---: | --- |
| Coreutils 9.12 | non-root shell tests | 493 | 0 | 121 upstream skips |
| Bash 5.3.20 | run-* groups | 81 | 5 | 0 |
| SQLite 3.49.2 | veryquick harness tests | 305880 | 0 | 1 explicit file; 12 upstream file/group notices |
| Busybox 1.37.0 | filtered assertions | 653 | 11 | 4 skips; 1 untested |
| GNU make 4.4.1 | portable categories | pending | pending | pending |

Bash uses the PTY helper with `timeout --foreground`, allowing interactive
children to remain in the foreground process group. The jobs group needs 300s
because of deliberate sleeps; other groups use 90s. The expected help table
uses the UTF-8 wide formatter, so only run-builtins starts in C.UTF-8. `/bin/sh`
in the utility kit stays Bash for Busybox's echo -ne fixture; only the Bash test
wrappers explicitly use dash. Unmodified same-version native Bash supplies
controls for the locale/platform comparisons.

Bash's five remaining raw comparison failures:

| Group | Classification | Evidence |
| --- | --- | --- |
| run-cond | unsupported libc feature | musl src/regex/regcomp.c rejects collating/equivalence classes with REG_ECOLLATE |
| run-glob-bracket | unsupported test prerequisite | compiles a guest C helper and loads strmatch.so; compiler/loadable builtin unavailable |
| run-glob-test | unsupported locale fixture | zh_TW.big5 and locale utility unavailable; native same-version control also fails |
| run-intl | unsupported locale fixture | legacy/numeric locales unavailable; native control also fails |
| run-execscript | upstream platform expectation / missing helper | upstream warns signal order varies; Linux SIGUSR1=10 precedes SIGTERM=15, unlike expected order; native control also fails; getconf ARG_MAX diagnostic additionally unavailable |

Bash run-test exposed the memfs bug: touch generated mtime later than atime
because each TimeSet::Now sampled the clock independently, so test -N failed.
The ticking-clock Rust regression is RED before the one-snapshot fix and GREEN
afterward. run-test and run-jobs both pass on the final rebuilt release CLI.

SQLite's original host-backed veryquick run reached its 1000-error cap:
four busy2 WAL failures and 996 date4 comparisons. busy2 passes 29/29 on
coherent guest tmpfs. HostFs's documented copied MAP_SHARED model cannot provide
cross-process WAL coherence; this is an unsupported mapping profile, not a new
kernel/exec contract introduced by validation. date4 assumes Linux means glibc
strftime(%k/%l/%P), whereas musl intentionally omits these GNU formats. SQLite's
SQL results are correct and the C-library reference returns empty strings.

The threaded-Tcl continuation passes 278577 assertions before sharedA-2.3
traps: upstream Tcl unix/tclUnixThrd.c:TclpThreadCreate casts void(ClientData)
to pthread's void*(*)(void*), incompatible with strict Wasm indirect-call types.
Classify this as an upstream native-ABI portability issue. Preserve the original
threaded binaries/log; use Tcl's supported --disable-threads profile, which
upstream run_thread_tests detects and skips naturally. No callback emulation or
excluded runtime feature was changed.

```sh
nice -n 19 taskset -c 4-7 timeout -k5 3000 bash tests/sqlite-10.sh "$CLI" >sqlite.log 2>&1
```

The final SQLite run reports 0 errors out of 305880, EXIT:0 and no allocation
leaks. Only date4.test is explicitly excluded through the upstream
SQLITE_TEST_PATTERN_LIST manifest; expected files are unchanged. Twelve upstream
skip notices cover atomic/atomic2 (f2fs), bigmmap (8GiB mapping on 32-bit),
multiplex3 (8.3 names build option), notify2/sharedA/sharedB/thread1/thread2/thread3
(non-threaded Tcl), zipfile/zipfile2 (loadable extension). The last thread warning
is a summary, not another skip. Silent ifcapable omissions are not enumerated
as skipped individual assertions. The test database cwd is guest tmpfs; source
and Tcl library are host-mounted, and real zoneinfo is mounted for localtime.

Busybox runs the explicit known and portable applet lists in tests/busybox-10.sh
against guest tmpfs. The known filter gives 198 PASS /11 FAIL /1 SKIP; portable
gives 455 PASS /0 FAIL /3 SKIP /1 UNTESTED. All eleven known failures match V10:

| Failure | Classification |
| --- | --- |
| awk gsub erroneous word start | upstream awk known bug without REG_STARTEND |
| cpio lists hardlinks | upstream test's documented known bug |
| hostname -i works | fixture compares IPv6 hostname reference with Busybox IPv4 output |
| mount remount,mand; mount RO loop | unsupported loop devices and Minix/ext2 filesystems |
| sed embedded NUL; NUL in command; nonexistent label | upstream documented SKIP_KNOWN_BUGS cases |
| start-stop-daemon both -x/-a | fixture uses multicall /bin/false rather than independent executable |
| strings works like GNU | GNU strings prerequisite unavailable |
| unzip subdir only | zip generator unavailable |

The source-level failure notes and original reproductions remain in userland-7.md.
Date timezone test passes. No additional kernel/libc bug was found in these
filtered assertions; no upstream expected-result file was changed.

GNU make source build passes. The inherited miniperl kit cannot load required
constant.pm, so userland-10.sh perl builds an ordinary static Perl 5.40.2 with
usedl=undef. probe-cc-10.py extends the existing actual-target launcher to preserve
the caller cwd and execute real Wasm directly during recursive guest build-tool
invocations. The owned prefix installs ordinary pure libraries and selected
static XS attributes/Cwd/Fcntl/IO/List::Util/POSIX. No probe result is fabricated.
tests/make-10.sh selects all features/functions/misc/options/targets/variables
categories except archives/load/loadapi, which need native compilation or
shared modules in the guest. VMS categories are outside this Linux profile.
GNU make runtime results are pending.
