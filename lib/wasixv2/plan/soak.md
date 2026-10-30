# V16 soak and leaks

Runtime base: `af8aa676d82` (`wasixv2/v16soak`). Recipe base: `59bd156af22`
(`wasixv2/plan-soak`). ABI `07cd32e`, tests `3a2d12d`, libc2 base `6bc05b3b`.

Design: reuse V9/V10/V11 source-built static asyncify binaries and the existing
guest workflow assertions in a persistent CLI machine, while sequential fresh
CLI machines repeat the same workloads. Collect minute host RSS, descriptor and
thread counts, guest task counts and a fixed published nginx HTTP probe; scan
all host output for panics continuously and retain bounded diagnostic tails.

Run from a dedicated systemd user unit, after other owned heavy work has ended:

```bash
systemd-run --user --collect --unit=v16soak-run \
  --property=AllowedCPUs=4-7 --property=MemoryMax=5600M \
  --working-directory="$HOME/dev/github.com/wasmerio/wasmer-wt/v16soak-plan" \
  bash -lc 'exec bash lib/wasixv2/plan/userland/soak.sh \
    "$HOME/dev/github.com/wasmerio/wasmer-wt/v16soak/target/debug/wasmer" \
    "$HOME/wasixv2-orch/logs/v16soak/run-01" 7200 \
    > "$HOME/wasixv2-orch/logs/v16soak/run-01.log" 2>&1'
```

No release build or full suite is required. The wrapper uses nice19, CPUs4–7 and
three compiler workers. The cgroup cap includes the host sampler and both CLI
processes; a 5500MiB aggregate RSS guard stops the run below the cap. The guest
uses tmpfs for coherent SQLite WAL mappings, fixed program images and bounded
per-cycle data/log files. The host keeps JSONL curves/events and 80-line output
tails, rather than traces of every syscall. Source kits are never removed.

Each guest cycle checks 100 HTTP responses with four concurrent clients during
nginx HUP; 200 SET and GET Redis benchmark requests with four clients and BGSAVE;
two WAL writers and a reader; eight-file recursive make-j4 rebuilds using the
pipe jobserver; 16 fork/execs and four seeded worker STOP/CONT/TERM/KILL lifecycles;
the original tmux controlling-PTY split/send/detach/attach/kill workflow; and FUSE
copy-up, whiteout and normal unmount/daemon exit. Final SQLite integrity is checked.
Workers are chosen deterministically from a recorded cycle seed; stop/continue
wait statuses and terminating signal statuses are checked explicitly.

The persistent guest runs at least7200s after startup. Fresh guests each run120s
and repeat throughout the same interval. Host samples are every60s; a separate
guest monitor counts `/proc/*/task` every60s, with additional quiescent counts at
cycle boundaries. A missing completed cycle for180s or startup for600s fails.
Probe requests must return exactly `soak-ok\n` within5s.

Growth flags compare five-sample block minima after five minutes of warmup,
requiring three nondecreasing blocks and net growth over32MiB RSS,8fds,4threads,
8tasks or10ms probe latency. These are investigation triggers, not proof of a
leak. Fresh-machine peak/end resources and quiescent counts must also be reviewed.

## Bounded panic finding

`HostBackend::drop` could run on its own reactor thread when an inbound callback
released the final network reference. Its unconditional native thread join then
panicked with `Resource deadlock avoided (os error35)`. The deterministic sibling
`reactor::tests::last_backend_reference_can_be_dropped_from_a_reactor_callback`
fails before production edits (`reactor-red.log`, EXIT101), catches the callback
panic, and also checks that the reactor's shared state disappears.

Runtime fix `b57e718dbf4` always stops/wakes the reactor and joins only from another
thread, matching the existing host timer lifetime handling. The reactor owns its
shared state until its stop loop ends. Linux source: `net/core/net_namespace.c`,
`cleanup_net`, which retires namespaces from the cleanup worker. No ABI or
kernel/exec contract changes. Ten focused reactor/publication/vsock checks PASS;
exact Makefile-flags all-target clippy and `cargo fmt --all --check` PASS.

Pilot02:5persistent+5fresh mixed cycles PASS, quiescent tasks11 throughout,
both exits0/DONE. The initial run01 was deliberately stopped after13.6minutes to
prove/fix the separate teardown interleaving:358cycles,6completed fresh machines,
quiescent11, no observed workload failure or panic. This partial run does not
count as two-hour validation. Original curves and the deterministic RED remain
archived.

Results: fixed-runtime full7200s confirmation pending. The analyzer
`userland/tests/soak-results.py RESULT_DIRECTORY` verifies actual persistent
runtime before writing `results.json`, reporting warm/late slopes, quantiles,
fresh-process peaks and quiescent task counts. No clean two-hour curve is claimed
yet. Canonical takeover state: `~/wasixv2-orch/logs/v16soak/STATE.md`.
