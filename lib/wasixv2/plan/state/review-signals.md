# WP9 (signals) state — stopped on request

Worktree: /home/theduke/dev/github.com/wasmerio/wasmer-review-signals
Pushed: wasmer `wasixv2/review-signals` (8d686147337 fix, 1f3950f419d wip submodule bump),
wasixv2-tests `review-signals` (0a9779e wip guest tests). Base bdcdb958bef (merge train NOT landed yet).

## Done (one commit 8d686147337, kernel unit tests in kernel/src/tests/signal.rs, all 195 kernel tests pass, clippy w/ lint RUSTFLAGS + fmt clean on kernel/exec/syscalls/wasix1/wasixv2)
- M-P6 on_sig_stack from __stack_pointer: UserMemory::stack_pointer (exec GuestMemory reads ExecEnv.stack_pointer, CompatMemory forwards); SignalFrame::stack_top field replaced by altstack_top(sp)/on_altstack(sp) (sigsp); kernel frames stack only holds saved uc_stack for restore_altstack; SS_AUTODISARM per signal_delivered; siglongjmp restores nothing. No Control trait change (Layered is codegen'd in wasixv2-abi).
- L-P3 altstack inheritance (TaskSignals::inherit(flags)).
- M-P4 RLIMIT_SIGPENDING per real uid (SigqueueUsers/Charge/Account in signal.rs; posix timers hold a Charge).
- M-P7 PR_SET_PDEATHSIG delivered in process_exited; cleared in commit_cred and privileged exec (one small edit in exec_commit's ident block).
- L-P1 leader exit status (thread::GroupState.leader_exit).
- L-P4 CLONE_PIDFD -> EINVAL.
- L-P5 pidfd_send_signal: zombie ok, info honoured (EINVAL/EPERM rules). Flags still EINVAL.
- M-P9 deferral only: exit_notify skips a vfork-parked leader (Kernel::vfork_waits). Killability while child runs NOT done (needs a Control method -> codegen in wasixv2-abi); documented in DESIGN.

## Remaining
1. Split 8d686147337 into per-fix commits (coordinator request) with their tests.
2. Run suite subsets (release CLI was built at target/release/wasmer before last rename; rebuild incremental): sig- proc- thr-, posix sig*/kill*/sigaltstack, ltp signal/prctl; report newly passing. New guest tests (sig-altstack-autodisarm, sig-altstack-inherit, sig-rlimit-sigpending, proc-pdeathsig, proc-vfork-parent-exit) are UNRUN — may need fixes.
3. Run exec/wasix1/syscalls crate tests.
4. Rebase onto merge train once `git log origin/feat/wasixv2 --grep 'atomic waits that signals interrupt'` hits; drop the wip submodule bump commit before merge as appropriate.
5. Follow-up: killable vfork parent (Control hook, codegen), pidfd_send_signal flags, CLONE_PIDFD implementation (needs GuestAction::Fork pidfd write).
