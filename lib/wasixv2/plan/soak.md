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
checking each contiguous run of at least three nondecreasing blocks and net growth over32MiB RSS,8fds,4threads,
8tasks or10ms probe latency. These are investigation triggers, not proof of a
leak. Fresh-machine peak/end resources and quiescent counts must also be reviewed.

After the long unit finishes, `userland/soak-host.sh RUNTIME_WORKTREE NEW_LOG 32`
builds a bounded standalone embedding driver against that debug CLI's existing
rlibs (no new dependency resolution), then repeats32full-workload machines in ONE
native process. It follows `api/tests/machine.rs`, caches only fixed compiled
images through the public Loader API, and asserts each machine's Net and
TokioHost weak references expire after shutdown/drop. `RETIRED` lines record
same-process fd/thread/RSS after every machine. This supplements the long CLI
run, since an OS process exit alone could hide machine-owned reference leaks.
Keep it serialized with the soak/build/gates and within the same cgroup cap.
Same-process result:32/32 full-workload machines PASS (`retirement-unit2.log`,
EXIT0). Every Net and TokioHost weak reference expired. Retirement fds stayed11
for all32; host threads stayed22 for the final16; RSS warmed from1085MiB to1363MiB
and varied by only4KiB over the final4retirements. Raw rows are retained in
`retirement-results.json`. This bounded check is separate from the required
continuous two-hour curves.

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

Fixed run02 was invalidated at~80minutes: its observer's minute ticks jumped from
722.46s to4802.69s (4080.23s gap), and no cycle progressed after774.997s. The agent's
requested60s clock wait also returned after4053.23s. Global user journal activity
continued, so neither whole-host suspension nor a guest deadlock is established.
The watchdog stopped the run; its failed summary/tails are preserved. New runs
explicitly reject observer-loop or sampling gaps>120s before classifying a guest
stall. No guest deadline/assertion has been weakened, and this interval is not
counted toward a successful two-hour result.

Run03 completed7203.55s afterREADY:1057persistent +1005fresh cycles across57fresh
machines (2062total),121minute sample ticks, largest gap60.83s. Every exit0/DONE,
all quiescent tasks11, no host panic/assertion/stall. Unit memory peak1.4GiB, no
swap. Its observer wrote small RAM-backed files under
`/run/user/1000/v16soak-run03`, archived into `logs/v16soak/run-03`.

This is a functional pass, **not a leak-free sign-off**: late native heap analysis
led to the separate retired-task leak below. Raw RSS was not flat. After five
minutes of warmup, the curves were:

| Resource | Median | p95 | Range | First/last15min median |
| --- | ---: | ---: | ---: | ---: |
| RSS KiB |601706|703112|533804–713596|562508 /696818|
| Native fds |73|77|65–79|72 /73.5|
| Native threads |40|47|31–50|38 /42|
| Guest tasks (in-cycle) |19|25|11–28|20 /17|
| Fixed HTTP probe ms |10.000|28.590|1.567–103.133|8.842 /4.141|

Second-half RSS slope was+131346KiB/hour. The original whole-run monotonic flag
was false after early decreases; inspection of the late curve correctly raised
an investigation. The corrected contiguous-run detector flags RSS and threads;
it passes four bounded controls, including an early-dip/late-growth regression
that failed first. Historical raw flags remain preserved separately from
`growth_flags_recomputed` in results. Thread counts overlap fresh controls; the
flag prompts review and is not proof of a native-thread leak. Quiescent guest tasks were exactly11 in every cycle.
`userland/tests/soak-results.py RESULT_DIRECTORY` verifies actual duration and
sample continuity before writing quantiles/slopes and fresh-process peaks to
`results.json`. Persistent/fresh cycle medians in corresponding10-minute windows
(excluding each machine's first two cycles) had ratios0.978–1.012. Both slowed
and recovered together under shared scheduling pressure; no aging-specific
throughput penalty was demonstrated. This is a shared-host robustness run.

### Retired task retention

Temporary read-only counters on the unchanged retirement logic found live task
and PID tables bounded, root asyncify saves5/~3–6KiB and retired credentials0–1,
but `DeadTask` records growing roughly once per cycle. A61s trace retained21dead
records across16cycles; every sampled retained task entered `release_task`
before its reaper unhashed it. Names span curl, tmux, nginx, SQLite and make,
which rules out a single guest program's cache as the explanation.

`release_task` checks `state.dead`, unlocks for fatal file/vfork cleanup, then
looks in `state.tasks` under a second lock. A concurrent wait/reap can move the
task into `dead` between these locks; the later absent-live-task return misses
that record. The bounded sibling regression
`tests::signal::reaping_concurrently_with_execution_release_leaves_no_dead_tasks`
races reap/release for4096exited children. RED0.33s/EXIT101 before production
changes; GREEN0.36s after. Fix `ff65b0255b5` consumes either live or retired
ownership atomically under the final table lock, preserving the existing
Control::release and exec contracts. Linux source: `kernel/exit.c:release_task`,
`kernel/fork.c:__put_task_struct`. 126focused signal/task/ptrace/FUSE tests PASS;
exact Makefile-flags all-target kernel clippy and workspace fmtcheck PASS.
Final fixed-runtime run04 completed7200.286s afterREADY:1579persistent and
1529fresh cycles (3108total),59fresh machines,120minute ticks/maxgap60.93s.
Every exit0/DONE, all quiescent guest tasks11, no panic/assertion/stall. This
confirms the established fixes functionally; **memory sign-off remains open**.
Machine-readable aggregates and evidence hashes are in `soak.results.json`.

Canonical takeover state: `~/wasixv2-orch/logs/v16soak/STATE.md`.

### Interpreting native RSS

The late RSS rise was investigated before classifying it as a leak. Supplemental
`quiescent.jsonl` samples host memory100ms after completed cycles, during the
guest's one-second idle. Version-specific `userland/tests/soak-arenas.py` reads
only the owned process's `/proc/PID/mem`, without attaching, pausing, signalling,
injecting calls or changing its allocator. Run it with read permission (normally
`sudo`) and the live result directory, optionally followed by a machine name.
Locked/changing arena snapshots are discarded and reported.

On this glibc2.39/x86_64 host, two stable55-arena snapshots at92.8/96.8minutes show
system bytes566.809/567.027MiB, free chunks529.830/529.756MiB and unfreed bytes
36.979/37.271MiB. A fresh-cycle control shows45arenas,321.055MiB system,
295.601MiB free and25.454MiB unfreed. Unfreed includes allocator metadata and
tcaches; it is not a precise application live-byte counter. Most native heap
memory is already free for reuse, so RSS alone does not establish a kernel leak.
No `malloc_trim`, arena-limit setting or runtime workaround has been applied.
Later stable55-arena samples through cycle1040 show system588.461MiB,
free548.905MiB and unfreed39.556MiB. The small unfreed increase was investigated
rather than dismissed as allocator retention, exposing the retired-task race
above. Raw RSS warming remains part of the reported curve.

Parser/layout source: glibc2.39
[`malloc.c:malloc_state,__malloc_info`](https://raw.githubusercontent.com/bminor/glibc/glibc-2.39/malloc/malloc.c)
and [`arena.c:heap_info,arena_get2,__malloc_arena_thread_freeres`](https://raw.githubusercontent.com/bminor/glibc/glibc-2.39/malloc/arena.c).
The latter retains detached arenas for reuse and bounds their creation using the
native core count unless explicitly configured. The heap-shaped map observation
is checked against that source; free-bin snapshots supply the stronger evidence.


`userland/tests/soak-tcaches.py` supplements the arena estimate for the exact
host libc SHA recorded in its guard. With no argument it validates calibration
on its own process; with a live result directory it reads native pthread cache
counts at a quiescent cycle. Linux
`kernel/futex/syscalls.c:sys_get_robust_list` supplies the thread descriptor
location, calibrated with our own `ARCH_GET_FS`. glibc
[`nptl/descr.h:struct pthread`](https://raw.githubusercontent.com/bminor/glibc/glibc-2.39/nptl/descr.h)
and the pinned local `__libc_malloc` disassembly resolve TLS;
[`malloc.c:tcache_perthread_struct,__libc_malloc`](https://raw.githubusercontent.com/bminor/glibc/glibc-2.39/malloc/malloc.c)
define the counts/size classes. The target's TLS offset is read from its own GOT,
not copied from the observer's address space. TCB self-pointers and duplicate
cache-count reads validate each thread; changing/disappearing threads are
reported as unstable. Self-control PASS; finalrun cycle805 measured33/33stable
threads and0.806MiB tcache chunks. This does not explain the entire arena
unfreed estimate. No process attach, target writes, signals or allocator tuning.


## Fixed run04 curves and open heap investigation

Canonical debug CLI `ff65b0255b5`, SHA256
`6597f726a69a8cf7ea4c1862c212af89dcf5bbeef8b109798959cbe480f75463`;
immutable guest/observer snapshot at recipe `8a7e2005439`. No full suite or
release build. Cgroup observed peak2060251136bytes, swap0, cap5600MiB;
process affinity/nice verified4–7/19. Raw data: `logs/v16soak/run-04`.

| Resource | Warm median | p95 | Range | First/last15min median |
| --- | ---: | ---: | ---: | ---: |
| RSS KiB |655904|776272|606140–795052|747852 /687500|
| Native fds |72|77|68–80|75 /73|
| Native threads |37|46|32–51|36 /39|
| Guest tasks (in-cycle) |18|25|11–27|15 /17|
| Fixed HTTP probe ms |2.835|12.552|1.279–19.897|2.038 /5.357|

RSS fell naturally around minute30, then rose again; late-hour slope+65301KiB/h.
Corrected block flags mark RSS/threads for investigation. The final100quiescent
host samples have fds71 and threads35–37; all3108quiescent guest counts are11.
Persistent/fresh cycle-median ratios are0.938–1.003 across10-minute windows, so
no aging-specific throughput loss is demonstrated.

The separate final32-machine embedding run PASS: every Net/TokioHost weak owner
expired, fds11 throughout, final16threads23–24, final4RSS1473644–1473652KiB
(8KiB variation). This checks many machines in one native process separately
from the long persistent-machine run.

Most native arena bytes are free for reuse, but the **unfreed estimate is not
flat**:26.230MiB at5min to38.823MiB at115min. Final arena sample53/53stable,
system575.832/free537.009MiB,35attached threads. Tcache64/85/115min estimates
0.806458/0.812927/0.821320MiB do not explain that increase.

A separate180s diagnostic run on `ff65` plus archived temporary read-only
counters completed51full cycles. DeadTask records stay0–1, live tasks11–29,
PID/process capacities56, root saved stacks4–6/~1–10KiB, tmpfs47–49inodes and
22–24blocks. All vfork tables are empty in sampled points. This confirms the
retirement race correction and rules out these simple accumulation paths in
the bounded run. Temporary sources were SHA-guarded/restored; production branch
stays clean. `counters-final-results.json` retains named ranges.

Native allocation profiling of a separate bounded fork/exec workload is in
progress using a task-owned extracted Valgrind package. Host packages and the
canonical CLI are unchanged. Do not call all resource curves flat or conclude
that the remaining heap rise is harmless before its allocation stacks are
understood. Canonical continuation state is the STATE.md linked above.
