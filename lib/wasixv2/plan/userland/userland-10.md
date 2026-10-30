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
already built; the upstream test driver, environment and expected comparisons
run in the guest. Each shell test has a 60-second bound. The driver preserves
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

Results are pending; do not interpret a successful source build or harness
launch as an upstream test pass. Every kernel/libc correction requires a
failing-first sibling regression and Linux source/function citation. Machine
handoff is `~/wasixv2-orch/logs/v12suites/STATE.md`.
