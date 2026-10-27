# Global rules for all wasixv2 agents (orchestrator, binding)

Copy of the orchestrator's rules, with this machine's paths replaced by what they stand for.

1. EXCLUSION (hard): do NOT touch dynamic linking, closures, libffi, or reflection logic in any repo
   (wasixv2-dylink, dl-* extensions, closure/reflection syscalls, libffi ports, function-table
   extensions owned by that team). Another team owns it. If you need it, at most stub (ENOSYS) and report.
2. Quality bar: no quick hacks, no one-off patches. Fixes must be comprehensive, sensible, respect
   lib/wasixv2/DESIGN.md, and follow Linux semantics (source: a checkout of torvalds/linux).
   Clean code matching surrounding style.
3. The v2 kernel stays clean: legacy/v1 quirks live ONLY in the v1 compat layer (lib/wasixv2/wasix1).
   Kernel behaviour settings only if generally applicable and sensible.
4. Do NOT take inspiration from the WASIX v1 implementation (lib/wasix, lib/virtual-fs, lib/virtual-net)
   for v2 design/code. The compat layer may READ v1 / wasix-org/wasix-libc code only to learn v1 ABI semantics.
5. Isolation: own git worktree + branch per agent; never edit other agents' worktrees. Put worktrees
   next to the main checkout (NOT /tmp — /tmp is wiped on reboot). Never delete
   another agent's worktree. Don't `git worktree prune` the main repo.
6. ABI changes land in wasixv2-abi first; wasmer and libc2 pin the SAME abi commit, regenerating in the
   bump commit (`WASIXV2_CODEGEN_BLESS=1 cargo test -p wasixv2-abi --test generated`).
   Only merge/bless agents edit wasixv2-tests expected-failures; fix agents report newly passing tests.
7. Push only if your prompt says so; fast-forward only, never force. Commit bodies end with
   `This commit was made by Claude Code (an AI agent) on behalf of the user.` and trailer
   `Co-Authored-By: Claude <noreply@anthropic.com>`.
8. Gates for crates you touch: cargo test, clippy (Makefile lint-packages RUSTFLAGS,
   `-D clippy::all --all-targets --locked`), `cargo fmt --all --check`.
9. Toolchain: `source lib/wasixv2/plan/env.sh` (first run `lib/wasixv2/plan/ensure-wasixcc.sh`).
   Suite: `WASIXV2_TESTS_WASMER=<abs wasmer> cargo run --release --locked --manifest-path tests/wasixv2/Cargo.toml -- [FILTER]`.
   Build wasmer with `cargo build -p wasmer-cli --features cranelift` (release for full suite runs).
10. Keep durable notes/logs in an orchestration directory outside the repositories (logs/<your-task>/), not /tmp.
11. Report concisely: branches/commits per repo, what changed, test results, follow-ups.
12. SHARED MACHINE — limit concurrency: cargo with `-j 3` (or CARGO_BUILD_JOBS=3, set by env.sh), make `-j3`,
    tests with RUST_TEST_THREADS=3 / `--test-threads 3`, the wasixv2 suite with at most 3 parallel jobs
    (check its runner for a jobs/threads option or env, e.g. RUST_TEST_THREADS). Only ONE full release
    build or full suite run at a time per agent; never run several heavy commands in parallel.
    Run every build/test/suite under `nice -n 19`. The suite's LTP build uses all cores
    (available_parallelism) — run the suite as `nice -n 19 taskset -c 0-3 cargo run ...` to confine it.
