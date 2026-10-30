# WASIX v2 performance validation (V14)

Status: measurements in progress. No speedup or completed validation is claimed
until the raw measurements and failing-first regression evidence are recorded below.

Runtime base: `7ccaad3e14e`, ABI `07cd32e`, tests `8ada114`, libc2
`6bc05b3b`. Runtime branch `wasixv2/v14perf`; tests branch `v14perf`; this
report is on `wasixv2/plan-perf`. The historical WASIX v1 runtime comparator is
the common ancestor `4a7f16c1154`, using the same v1-built programs. Running a
v1-built program on the v2 runtime is separately labelled a v1 **ABI** comparison.

Method: release CLI with Cranelift and LLVM 22.1.8, release debug information for
host profiling; `nice -n 19 taskset -c 4-7`, three build jobs, incremental off.
Five measured runs after warmup, alternating configuration order. Report medians
and min–max ranges; retain commands, raw results, statuses and concurrent load.
The merge train runs the full suite on CPUs 0–3 and other implementation agents
use CPUs 4–7, so this is a shared-machine baseline, not a dedicated-host claim.
No full suite is run by V14. Host perf uses `sudo -n perf`; unprivileged perf is
disabled by the host's paranoid setting, and that setting is unchanged.

Micro sources extend the existing tests repo `bench/syscalls` layout at
`bench/performance`: same C source, native Linux, v1 asyncify, v2 EH and asyncify.
Timings are internal to the process, excluding CLI startup and compilation.
Native files use `/dev/shm`, guest files use `/tmp` tmpfs. Native clock_gettime
can use the vDSO. Workload definitions, byte accounting and caveats are in that
directory's README. EH fork is explicitly unsupported, not a timed failure.

Macro build/run recipes live in [perf/](perf/). They reuse userland sources
from `wasixv2/plan-userland7` (`d83c8339ff6`) and
`wasixv2/plan-userland10` (`fd92a202ea7`), without changing or deleting those
source kits. SQLite 3.49.2 speedtest1 is compiled with the same feature flags for
native/v2/v1. Native Redis 7.0.15 and nginx 1.28.0 are rebuilt from the same
archives as the preserved v2 binaries. SQLite reports its internal total;
Redis reports SET/GET request rates; nginx uses a validating HTTP/1.1 keepalive
load generator with 16 clients and 2 server workers; shell/coreutils reports
end-to-end wall time over 20 deterministic pipeline/file cycles. CPU pinning
includes the load generator. The shell workload uses the source-built v2
utilities and native system utilities, so their differing userspace versions
are a declared comparison limitation.

Raw logs, binary/source manifests and handoff:
`~/wasixv2-orch/logs/v14perf/STATE.md`. No prebuilt Wasm is committed.
