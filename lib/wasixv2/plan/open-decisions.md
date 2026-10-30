# Open decisions for WASIX v2

These four questions need a decision from the project owner. Until then they block the remaining
expected failures and some follow-up work. The state is as of `feat/wasixv2` `f2e457967c8` (merge
train 22): about 4076 tests pass, 87 are expected failures and 436 are ignored.

## 1. Host-enforced page protections (and userfaultfd)

**Question.** Should a guest load or store into memory that is `PROT_NONE`, unmapped or read-only
raise `SIGSEGV`/`SIGBUS`, as on Linux?

**Today.** The kernel records page protections and enforces them in its own copies: since train 19,
kernel copies return `EFAULT` on the low guard. The guest's own loads and stores into linear memory
are not checked, because wasm has no per-page protection.

**What it would take.** About two weeks of risky work in the exec memory layer:
- a protection hook in `Control`;
- re-protection across fork and copy-on-write;
- a per-call cost on `mprotect`;
- mapping engine traps to signals.

**What it unblocks.**
- About 16 named tests: 9 LTP, including `shmdt01` (listed twice), and 7 Open POSIX `mmap`/`munmap`
  tests.
- `select03`.
- Exact `mtime`/`ctime` on writes through shared file mappings. Today those times are marked at
  `msync`/`munmap`, inside the window POSIX allows, as a documented deviation.
- `userfaultfd`, which needs page-fault interception.
- Private copy-on-write semantics for `mmapstress04`.

**Options.**
- **Go:** schedule it as its own work package. It changes the kernel/exec contract.
- **No-go:** document these as wasm limits and move the tests to `excluded.txt`.

## 2. Record `PROT_EXEC` without enforcing it

**Question.** `mmap`/`mprotect` with `PROT_EXEC` fail with `EACCES` today, a stance recorded in
`DESIGN.md`. Should the kernel accept the flag, record it (it would show in `/proc/<pid>/maps`) and
document that it is not enforced?

**Context.** Wasm cannot execute data, so `PROT_EXEC` can never really take effect. Many programs
and about 7 LTP tests request it routinely, for example JITs probing for support and loaders.

**Trade-off.** Accepting it makes those programs and tests work. It reverses an earlier `DESIGN.md`
decision, and it could mislead a program into thinking it can run generated code. Such a program
would then fail later, at the indirect call, instead of at `mmap`.

**Recommendation.** Accept and record it, and document the limit. This is not done, pending
agreement.

## 3. Memory accounting

**Question.** Resume the memory-accounting work, which the user stopped on 2026-10-28, and decide
what happens to its branch.

**Context.**
- The branch `wasixv2/mem-account` exists only locally, in the worktree `wasmer-wt/memacct`; it was
  never pushed.
- Without memory accounting, the overcommit policy, `min_free_kbytes`, cgroup `memory.max` limits
  and the OOM-related tests are recorded but not enforced.
- `ltp/mm/min_free_kbytes` is listed as an expected failure for this reason: it allocates until the
  2 GiB test scope kills it.
- The design is in the orchestrator notes, `design-memory.md`.

**Options.**
- Push the branch and resume the work.
- Keep it local and paused.
- Drop it.

## 4. Two changes outside WASIX, in wasmer's core crates

**Question.** Keep these two changes, which landed on `feat/wasixv2`, or move them out?

1. **`SharedMemory::host_data()`** (`lib/api`, P2, train 15). A public method that returns a shared
   memory's host base address and current length without a `Store`.
   - **Why it exists:** the kernel needs synchronous access to another process's memory, for
     ptrace, `process_vm_readv`/`writev` and `/proc/<pid>/mem`, and `wasmer::Store` is not `Send`.
   - **Safety:** after the review fixes in train 18 it returns `None` unless the memory's
     reservation can never move. All `unsafe` use stays inside the exec layer, which bounds-checks
     every access.
2. **Bulk-memory range fix** (the VM, P3, train 22). Wasm bulk-memory operations on ranges ending
   exactly at 4 GiB were wrongly rejected. The wasm 2.0 spec allows them. The fix has failing-first
   tests.

**Options.**
- Keep both and upstream them through the normal wasmer review.
- Split them into separate PRs against wasmer `main`.
- Rework `host_data()` behind a feature gate or a WASIX-internal crate.
