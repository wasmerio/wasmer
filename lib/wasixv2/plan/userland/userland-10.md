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
Attempt static Tcl with its upstream `--disable-shared --disable-load` options;
if unsupported, use SQLite's Tcl-free fuzzcheck alternative. No linking,
closures, libffi or reflection source changes are in scope.

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

Results are pending; do not interpret a successful source build or harness
launch as an upstream test pass. Every kernel/libc correction requires a
failing-first sibling regression and Linux source/function citation. Machine
handoff is `~/wasixv2-orch/logs/v12suites/STATE.md`.


Runtime fixes pushed by the AI agent with the required attribution:
`87cba5c4b5c` (host symlink times), `3d7fcca51fe` (resumed ptrace signal ordering).
Linux references: `fs/utimes.c:do_utimes_path/vfs_utimes` and
`kernel/signal.c:get_signal/ptrace_signal`. No ABI or kernel/exec contract change.
Host driver gates: 15 tests. Kernel gates: 33 ptrace +34 signal tests. No full suite.


Bash cross configuration needs an actual target fork/wait probe:
`tests/probes/wait-status-10.c` prints the WEXITSTATUS offset (8 on the tested
Linux ABI). `userland-10.sh bash` runs it through `WASIXV2_PROBE_CLI` and passes
its measured result to configure. The upstream cross fallback 0 corrupts
lastpipe's synthetic process statuses. This is a build configuration correction,
not a kernel change. Use the source-built dash as `/bin/sh` for POSIX wrapper
scripts; Bash POSIX mode exits on the suite wrapper's `unset` of readonly UID.
For tty tests, `run-shell-suite-10.py CLI --pty --label bash-pty` supplies an
isolated host PTY as the CLI's ordinary guest console, preserving expected files.
Initial no-tty/raw baseline was 75 PASS / 11 FAIL across 86 groups.

Tcl's first static link selected compatibility memcmp/strstr/strtoul from
unknown cross-run results and failed duplicate strtoul. The typed runtime probe
`tests/probes/tcl-compat-10.c` checks the same upstream conditions on the actual
target. Only after it passes does the recipe cache the successful results.
Core `binaries libraries` and corresponding install targets avoid bundled shared
extension packages. Static Tcl/SQLite results remain pending.
