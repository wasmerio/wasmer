# WASIX v2 performance — V14

Cranelift baseline, five repeated runs after warmup; seven interleaved before/after pairs for retained changes. Values below are measurements on a shared host, not dedicated-host capacity claims. LLVM was attempted but its final link fails because the local LLVM 22.1.8 has only the WebAssembly target and lacks native target initialization symbols. No LLVM timing is invented.

Runtime base `7ccaad3e14e`; legacy v1 runtime common ancestor `4a7f16c1154`; ABI `07cd32e`; libc2 `6bc05b3b`; initial tests `8ada114`, benchmark source commits `5dc6e22` and `374d353`. Runtime branch `wasixv2/v14perf`, tests branch `v14perf`, plan branch `wasixv2/plan-perf`. No ABI or libc2 changes.

## Method

Ubuntu host, Linux 6.8.0-117-generic, AMD Ryzen 9 9950X exposed as 8 vCPUs, about 15 GiB RAM; Rust 1.96, wasixcc 0.4.7, Binaryen 129. All builds and measurements use `nice -n 19 taskset -c 4-7`, three build jobs and `CARGO_INCREMENTAL=0`; the requested make workload alone uses `-j4`. Own heavy commands are serial. Other agents and the merge train run concurrently; raw data records load/process activity. The final phase overlaps V15 validation. Micro A/B alternates runtime order each repetition. No full suite was run by V14.

CLI builds are release with debug information for profiling. Micro timings use guest/native monotonic clocks inside the process, excluding CLI startup and compilation. Module caches are warmed first. One source compiles for native Linux, v1 asyncify, v2 EH and v2 asyncify at `-O2 -g`. All three Wasm variants import a shared memory with 129 initial 64 KiB pages and maximum 65536 pages; source/module size and fork-copy strategy still differ. Native clock_gettime can use the vDSO. `getpid` is a libc call; it is not an isolated assembly-instruction timing.

Native cached-file tests use `/dev/shm`; guests use `/tmp` tmpfs. Positioned file tests overwrite/read offset 0 and report combined bytes in both directions. Pipe/TCP bulk transfers use 64 KiB operations and a validating receiver thread; creation is outside the interval, completion/join inside. Pipe capacity and guest in-memory TCP differ from Linux host transport. Poll watches 256 pipes with one ready; epoll uses maxevents=1 with one/all 256 ready. Mapping tests map 64 KiB, check zero endpoints, touch and unmap. Process timings include exit/wait and the actual loaded image; EH fork is unsupported and uses asyncify. Every timed operation checks its result.

**Comparison labels matter:** v1 ABI on v2 uses the new kernel and translation layer; it is distinct from the historical v1 runtime. The legacy CLI needs `--net`; initial TCP bind failures without it were harness errors, corrected and remeasured. Legacy epoll succeeds in the one-call warmup but fails repeated level-trigger readiness assertions, so it has no measured value. All other micro configurations used in tables passed.

Raw command/output/status/load records and perf recordings remain in `~/wasixv2-orch/logs/v14perf`; compact machine-readable summaries are in [perf/results](perf/results). The native scheduler-sensitive results have wide ranges, including TCP connection 40.7 µs–5.32 ms, so no narrow statistical confidence is claimed. Median and min–max are reported, not standard errors.

## Baseline latency

All cells are **µs/operation**, median [min–max], five measured runs. Full workload definitions and byte accounting are in tests `bench/performance/README.md`.

| Operation | Native Linux | Legacy v1 runtime | v1 ABI on v2 | v2 EH | v2 asyncify |
| --- | --- | --- | --- | --- | --- |
| getpid | 0.058 [0.038–0.071] | 0.147 [0.097–0.218] | 0.975 [0.919–1.002] | 0.888 [0.579–1.122] | 0.854 [0.733–1.067] |
| clock_gettime | 0.019 [0.019–0.022] | 0.287 [0.223–0.370] | 0.877 [0.809–0.976] | 0.714 [0.700–0.719] | 0.716 [0.710–0.859] |
| pipe_small | 0.213 [0.184–0.351] | 1.873 [1.399–3.113] | 3.945 [3.030–4.030] | 2.610 [2.458–3.852] | 2.735 [2.541–2.815] |
| file_small | 0.243 [0.237–0.294] | 1.913 [1.307–3.230] | 4.768 [4.649–6.694] | 4.630 [4.152–4.804] | 4.598 [4.145–4.809] |
| pipe_bulk | 7.708 [6.011–16.719] | 22.899 [14.202–40.727] | 45.004 [37.968–47.857] | 47.652 [34.789–58.627] | 47.630 [44.925–53.768] |
| file_bulk | 2.747 [2.622–4.335] | 5.043 [1.783–13.835] | 10.508 [9.838–18.836] | 12.043 [10.116–15.853] | 10.189 [10.094–28.682] |
| fork_exit | 300.326 [135.601–384.538] | 5964.012 [4179.864–11015.833] | 787.734 [406.196–1457.092] | unavailable | 1522.857 [1186.820–2210.800] |
| fork_exec | 382.739 [294.638–392.564] | 13395.602 [8524.208–20140.186] | 1543.049 [1072.862–1806.960] | unavailable | 2790.724 [2370.744–3650.048] |
| vfork_exec | 741.223 [237.441–2213.728] | 4374.969 [2373.419–4997.694] | 2325.947 [2188.229–2724.873] | 2121.438 [1778.437–2747.957] | 2292.986 [1412.189–2831.112] |
| thread | 462.268 [192.953–1085.579] | 2122.308 [1635.844–2318.495] | 1336.091 [694.673–1566.069] | 1807.580 [1621.856–2328.561] | 2319.041 [1896.172–3362.291] |
| futex | 7.506 [3.477–34.711] | 12.345 [11.829–16.089] | 13.476 [9.286–43.841] | 24.371 [13.898–52.622] | 23.991 [20.639–40.478] |
| poll_many | 7.766 [5.714–10.968] | 73.245 [68.386–137.537] | 94.957 [85.130–135.247] | 59.308 [44.032–94.980] | 55.883 [31.455–61.790] |
| epoll_many | 0.090 [0.089–0.098] | unavailable | 26.239 [19.872–35.463] | 18.503 [13.782–32.261] | 23.934 [16.676–37.038] |
| epoll_ready | 0.093 [0.092–0.100] | unavailable | 25.718 [21.569–55.097] | 36.198 [31.308–46.152] | 29.014 [24.291–36.037] |
| tcp_bulk | 19.187 [11.829–32.355] | 14.032 [12.419–14.689] | 188.676 [148.743–301.052] | 186.907 [100.205–327.769] | 180.990 [136.743–274.659] |
| tcp_connect | 5254.031 [40.734–5315.299] | 22.902 [22.309–24.885] | 49.038 [18.187–68.966] | 64.591 [16.516–131.239] | 70.311 [31.866–74.214] |
| mmap | 7.057 [3.579–8.143] | 0.324 [0.305–0.348] | 0.328 [0.309–0.719] | 10.844 [8.877–14.474] | 13.447 [9.787–16.067] |

## Cached/buffered throughput

MiB/s, median [min–max], derived from latency samples. Baseline cells use five runs and final cells seven. `file_bulk` counts 128 KiB per write/read pair; pipe/TCP count 64 KiB delivered. These are resident-buffer rates, not disk throughput.

| Operation | Native | Legacy v1 | v1 ABI on v2 | v2 EH | Final v2 |
| --- | --- | --- | --- | --- | --- |
| pipe_bulk | 8108.0 [3738.2–10398.0] | 2729.4 [1534.6–4400.9] | 1388.8 [1306.0–1646.1] | 1311.6 [1066.1–1796.5] | 2432.2 [1812.0–2687.0] |
| file_bulk | 45502.7 [28837.6–47674.9] | 24786.5 [9035.2–70124.5] | 11895.8 [6636.4–12705.6] | 10379.9 [7885.1–12356.9] | 13385.7 [13048.2–13738.2] |
| tcp_bulk | 3257.4 [1931.7–5283.8] | 4454.0 [4254.8–5032.7] | 331.3 [207.6–420.2] | 334.4 [190.7–623.7] | 10374.4 [3719.3–11852.4] |

## Five largest valid v1 regressions and profiles

After correcting legacy networking, the five largest Cranelift v2/legacy median ratios are mmap 33.44×, TCP bulk 13.32×, getpid 6.04×, TCP connection 2.82× and clock_gettime 2.49×. Ratios compare the initial baseline phases; causal fix gains use the interleaved A/B below. Five-second host-process recordings use `sudo -n perf record -F 99 --call-graph dwarf, 16384`; target runs as the ordinary user. The host paranoid setting is unchanged. Full stacks are retained; self-frame summaries disable inline expansion because the host addr2line decoder failed on some debug records. Guest JIT addresses may be unsymbolized.

| Workload | Highest self frames in the baseline recording |
| --- | --- |
| mmap | 9.56% `native_flush_tlb_one_user`; 5.38% `SyscallEntry::call`; 3.32% `malloc`; 3.32% `wasixv2_kernel::Kernel::mm`; 3.32% `AsTask::poll` |
| tcp_bulk | 37.78% `Vec::from_iter (temporary byte vector)`; 34.27% `RecvQueue::read`; 5.22% `native_write_msr`; 5.17% `contended host mutex`; 1.54% `wasmer::backend::sys::entities::memory::MemoryBuffer::write` |
| getpid | 9.89% `malloc`; 8.45% `AsTask::poll`; 8.44% `corosensei host-stack wrapper`; 3.49% `wasixv2_exec::caller_parts::_$u7b$$u7b$closure$u7d$$u7d$::h5090c03d0b838072`; 3.49% `wasixv2_kernel::Kernel::process` |
| tcp_connect | 7.50% `try_to_wake_up`; 6.32% `pids_can_fork`; 3.86% `<wasixv2_kernel::timer::Timer as core::ops::drop::Drop>::drop`; 3.86% `typed async host import`; 3.67% `wasixv2_vnet::stream::Conn::read` |
| clock_gettime | 9.13% `corosensei host-stack wrapper`; 8.12% `AsTask::poll`; 7.84% `malloc`; 5.44% `wasixv2_kernel::ns::<impl wasixv2_kernel::Kernel>::task_ns`; 3.54% `wasmer::backend::sys::entities::function::finalize_dynamic_call` |

TCP collection and bytewise receive copies consumed about 72% of initial bulk samples. These are removed with safe slice copies under the existing connection mutex. The ready-waker owning allocation is removed on immediately-ready calls. Mmap still spends substantial host time in page release and cross-CPU TLB invalidation; v1's heap-style mappings are not Linux VMA/protection semantics. The implementation retains required zeroing/protection/shared-page invariants. No raw memcpy replaces the engine's intentional volatile guest-memory copies.

## Fixes and failing-first evidence

| Change | Red proof | Final behavior |
| --- | --- | --- |
| Epoll scan serialization and deque reuse (`82b9da9f9a3`) | Competing harvests deliver 2 one-shot events instead of 1 | One-shot disarm serialized with harvest/control; callbacks remain independent; no full ready-list copy/rebuild per maxevents=1 call |
| Lazy I/O cursor (`6f4414571ff`) | Stopped copy consumes all later iovecs | Unvisited pieces remain and scalar/vectored copies allocate no pieces vector |
| Panic-safe current task (`602f695e1a1`, corrected by `b6009d713cb`) | Caught inner unwind leaks TaskId(7) into outer TaskId(3) | Drop restores the outer scope; large futures stay boxed |
| Borrowed ready waker (`b1ca09fee04`) | Ready poll owns 3 executor references instead of 2 | Stack-lived forwarding during poll; clones lazily own a cached standard Arc; escaped clones, wake races and nesting pass |
| TCP slice copies (`edf13c7a8bb`) | Warmed transfer allocates 1 temporary buffer | Zero temporary allocations; wrapped queues, corking, urgent marks and peeks preserved |

Linux references are in code/commits: `fs/eventpoll.c:ep_send_events/ep_start_scan/ep_done_scan`, `lib/iov_iter.c:iterate_iovec`, `include/linux/sched.h:current`, `kernel/sched/core.c:try_to_wake_up`, and `net/ipv4/tcp.c:tcp_sendmsg_locked/tcp_recvmsg_locked`. No ABI or kernel/exec interface changed; v1-specific behavior stays in the compat layer.

The inline-future allocation experiment was **rejected**: seven interleaved getpid medians 588ns before versus 866ns after, approximately 47% slower; clock_gettime about 45% slower. `b6009d713cb` restores the compact boxed representation. The rejected experiment is retained only as evidence in `micro/ab.*` and `bin/wasmer-fixed`; it is not the final implementation.

## Retained-change A/B

Seven interleaved measured runs per runtime, same v2 binaries and CPU pool; fork cases use asyncify, other cases EH. Values are µs/operation. A ratio below 1 is faster. This is the final implementation compared directly with the integration base, not the rejected inline experiment.

| Operation | Before median [min–max] | Final median [min–max] | Final/before |
| --- | --- | --- | ---: |
| getpid | 0.592 [0.569–0.628] | 0.548 [0.536–0.573] | 0.926 |
| clock_gettime | 0.699 [0.677–0.710] | 0.658 [0.651–0.675] | 0.942 |
| pipe_small | 2.495 [2.458–2.596] | 2.290 [2.274–2.315] | 0.918 |
| file_small | 4.164 [4.123–4.192] | 3.980 [3.936–4.016] | 0.956 |
| pipe_bulk | 34.445 [31.259–40.104] | 25.697 [23.260–34.492] | 0.746 |
| file_bulk | 9.840 [9.369–10.112] | 9.338 [9.099–9.580] | 0.949 |
| fork_exit | 733.116 [676.205–945.085] | 840.228 [679.894–1043.917] | 1.146 |
| fork_exec | 1646.457 [1498.104–2119.894] | 1700.783 [1466.078–1951.139] | 1.033 |
| vfork_exec | 953.547 [798.795–1017.903] | 939.048 [835.691–1059.153] | 0.985 |
| thread | 710.258 [561.708–824.447] | 596.126 [500.891–790.084] | 0.839 |
| futex | 3.512 [1.782–7.960] | 5.269 [2.213–8.069] | 1.500 |
| poll_many | 27.913 [26.543–31.268] | 26.790 [26.236–29.911] | 0.960 |
| epoll_many | 9.521 [9.139–10.379] | 9.531 [9.084–10.186] | 1.001 |
| epoll_ready | 14.100 [13.708–16.045] | 10.155 [9.184–17.574] | 0.720 |
| tcp_bulk | 89.123 [87.160–101.115] | 6.024 [5.273–16.804] | 0.068 |
| tcp_connect | 7.919 [7.007–8.703] | 7.535 [6.939–8.482] | 0.952 |
| mmap | 5.186 [5.100–5.377] | 5.320 [5.122–6.136] | 1.026 |

The short futex case had a noisy +50% median (before 1.78–7.96µs, after 2.21–8.07µs). A longer confirmation uses **11 interleaved pairs of 400000 rounds**: before 8.523µs [6.990–10.739], after 8.860µs [6.970–11.524], ratio 1.040. It does not confirm the 50% regression; broad overlap leaves a small blocking-path cost unresolved. Both observations remain visible, and no futex speedup is claimed.

The clearest retained micro gains are TCP bulk **14.8×**, all-ready epoll **28%**, pipe bulk **25%**, getpid **7.4%** and clock_gettime **5.8%**. Fork/exec, thread and futex scheduling ranges overlap; mmap is 2.6% slower in this A/B phase and its large legacy regression is not fixed.

## Macro workloads

Recipes [build-macros.sh](perf/build-macros.sh), [macros.py](perf/macros.py), [core.sh](perf/core.sh), [parallel.mk](perf/parallel.mk) reuse preserved userland recipes from plan-userland7 `d83c8339ff6` and plan-userland10 `fd92a202ea7`. SQLite 3.49.2 speedtest1 is compiled native/v1/v2 with the same feature flags; size 5, THREADSAFE=1, no loadable extensions, memory and tmpfs variants. Redis 7.0.15 uses 20000 SET/GET operations, 16 clients. Nginx 1.28.0 uses 2 workers, 16 persistent HTTP/1.1 clients, 5-second intervals and validated 4608-byte responses through a published host port; native clients share CPUs 4–7 with the server. Sendfile/access logging are disabled; document root is host-backed. Core workload runs 20 file/pipeline/fork-exec cycles on tmpfs. Guest Bash 5.3/coreutils 9.12 differ from native Bash 5.2.21/coreutils 9.4, a declared limitation.

Five measured runs follow warmup. SQLite cells are its internal total seconds; Redis/HTTP are requests/s (higher is better); core and make are end-to-end seconds. Baseline macros and final verification are separate shared-load phases, so changes between those columns are **not** claimed as causal speedups.

| Macro / unit | Native | Legacy v1 | v1 ABI on v2 | Baseline v2 | Final v2 |
| --- | --- | --- | --- | --- | --- |
| SQLite memory / s | 0.039 [0.038–0.042] | 0.301 [0.295–0.312] | 0.234 [0.221–0.239] | 0.226 [0.224–0.230] | 0.234 [0.222–0.254] |
| SQLite tmpfs / s | 0.135 [0.118–0.193] | 1.110 [0.716–1.245] | 1.197 [0.672–1.458] | 1.065 [0.835–1.276] | 0.338 [0.327–0.354] |
| Redis SET / req/s | 87719.300 [59171.600–132450.340] | unavailable | unavailable | 21762.790 [15491.870–25575.450] | 69444.450 [61538.460–89686.090] |
| Redis GET / req/s | 96618.360 [81967.210–122699.380] | unavailable | unavailable | 26845.640 [16638.940–32573.290] | 68728.520 [50125.320–95693.770] |
| nginx / req/s | 79426.730 [77560.907–80003.229] | unavailable | unavailable | 15838.758 [12930.350–17322.424] | 9920.450 [6851.699–13073.339] |
| shell/coreutils / s | 0.644 [0.350–0.742] | unavailable | unavailable | 5.048 [4.417–5.324] | 1.308 [1.153–1.620] |
| zlib make-j4 / s | 1.061 [0.993–1.122] | unavailable | unavailable | unavailable | unavailable |

The make workload is a real 15-translation-unit zlib 1.3.1 compilation plus `libz.a`, fresh host tmpfs outputs each time, `make -j4`, native clang 22 emitting wasm32 objects and packaged guest Clang 16. Native build succeeds and verifies 15 objects/archive. Guest Clang rejects its own `-cc1` dispatch on **both** legacy and v2, full and slim atoms. A second pinned package has byte-identical atoms. The guest build is therefore **blocked by the compiler package**, not reported as a successful performance result; no synthetic replacement, loader/reflection changes or prebuilt Wasm commits. [prepare-build.sh](perf/prepare-build.sh) pins package/archive hashes and reproduces the attempt.

Initial `--inmem` SQLite option failures were recipe errors (`--memdb` is correct); initial nginx clients hit the default 1000-request keepalive limit. Both were corrected and repeated successfully. Initial raw failures remain retained and are excluded from valid result summaries.

## Validation and remaining work

Final gates: 376 distinct unit tests (wait 31, vnet 143, syscalls 10, filtered kernel 192), all-target clippy for all 4 touched crates using exact Makefile lint flags, and `cargo fmt --all --check`. No full WASIX/workspace suite. The integration train owns full-suite validation. Release CLI, micro A/B and macro verification evidence are archived with source/ABI/tests/libc identifiers and SHA256. No original userland kit or another agent's worktree/target/cache is deleted.

Known gaps: LLVM timings require a native-target LLVM 22 build; guest compilation requires a working Clang package; legacy epoll has no valid repeated level-trigger comparison; same-source v1 Redis/nginx/core/make macros were not built. Mmap remains a large v1 regression dominated by required host page/TLB work. Universal async host-import/stack-switch cost and task-table locking remain in small syscalls; the exact kernel semantics are retained. Further fast-import or page-management mechanisms need design review, and any kernel/exec contract change must be reported before implementation. This package does not claim those unresolved costs are fixed.

## Reproduction

```sh
export BINARYEN=$HOME/wasixv2-tools/binaryen CARGO_INCREMENTAL=0
source lib/wasixv2/plan/env.sh
export WASIXCC_SYSROOT_PREFIX=$HOME/wasixv2-tools/libc2-v11progs/build-wasixv2
export BENCH_OUT=$HOME/wasixv2-orch/logs/v14perf/micro
bash tests/wasixv2/bench/performance/build.sh
python3 tests/wasixv2/bench/performance/run.py /abs/release/wasmer --out "$BENCH_OUT" --backends cranelift
python3 lib/wasixv2/plan/perf/ab.py /abs/before /abs/final --out "$BENCH_OUT"
bash lib/wasixv2/plan/perf/build-macros.sh
python3 lib/wasixv2/plan/perf/macros.py /abs/release/wasmer --out "$HOME/wasixv2-tools/userland-v14perf" --configs native cranelift --workloads sqlite sqlite_tmpfs redis nginx core
```

The report and source-only recipes are committed on plan-perf. Exact per-repository commit inventory, chronological red/green evidence, limitations and final cleanup are in `~/wasixv2-orch/logs/v14perf/STATE.md`.
