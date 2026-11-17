# Apple M5 Max call benchmarks

The published import changes raised ready typed guest-to-host throughput from 6.8 to 17.1 million/s on this Mac. The scalar-result prototype moved dynamic host-to-guest calls from 32.86 to 29.13 ns. Wasmtime’s typed export took 11.78 ns.

The Apple M5 Max ran the same Rust fixtures as the [AMD benchmark](https://github.com/wasmerio/wasmer/actions/runs/44197172290). Both suites used Wasmer LLVM 23.1.5 and Wasmtime 50.0.1 Cranelift through their native Rust APIs. Rust 1.99.1 compiled Mach-O arm64 release binaries. The operating system scheduled the processes on this 18-core Mac. The local V8 timing and Rust release builds finished before the Rust timings began.

The **Base** commit contains the original #7195 implementation. **Imports** adds the published import optimizations. **Scalar** adds the one-result dynamic export prototype to Imports. [Base](https://github.com/wasmerio/wasmer/commit/c2366fe3c70acc8d713540051efc0203c5ee8311) · [Imports](https://github.com/wasmerio/wasmer/commit/acb8542694cd3970d96e39088b8873dfe8cc627c) · [Scalar](https://github.com/wasmerio/wasmer/commit/d3b2953ef5354ecc55a04e6f1ed58df3aba28798) · [Frozen Rust fixtures and lock](https://github.com/wasmerio/wasmer/tree/509ba1e0a25ea00e5f2ef5c676a0a45d635627d5/benches/x64_calls)

Each table reports median wall-clock nanoseconds per operation. Parentheses show millions of operations per second. Each of eight blocks measures all four arms in a position- and carryover-balanced order. The separate native control runs all six orders of the three Wasmer executables. Each estimate uses 24 Criterion samples after 0.2 s warm-up and a 0.4 s measurement target. Each revision uses a fresh Wasmer API, VM and LLVM build in its own target directory. The Wasmtime column always uses the baseline binary.

## Rust host → Wasm

A row measures one complete outer call using a cached handle. Suspended calls return Pending once and resume. Dynamic Wasmer exports return owned values. Wasmtime dynamic exports use caller-provided result storage.

| Case | Base | Imports | Scalar | Wasmtime |
| --- | ---: | ---: | ---: | ---: |
| Typed | 15.26 (65.6) | 15.49 (64.6) | 15.32 (65.3) | 11.78 (84.9) |
| Dynamic | 32.90 (30.4) | 32.86 (30.4) | 29.13 (34.3) | 63.42 (15.8) |
| Typed ready | 100.78 (9.9) | 101.51 (9.9) | 101.24 (9.9) | 92.51 (10.8) |
| Dynamic ready | 104.09 (9.6) | 105.05 (9.5) | 100.47 (10.0) | 152.75 (6.5) |
| Typed suspended once | 234.61 (4.3) | 215.51 (4.6) | 216.81 (4.6) | 225.77 (4.4) |
| Dynamic suspended once | 236.44 (4.2) | 217.83 (4.6) | 219.48 (4.6) | 286.76 (3.5) |
| Guest → host add, including outer call | 27.91 (35.8) | 25.59 (39.1) | 26.31 (38.0) | 13.67 (73.2) |
| Typed reentry, one level | 52.41 (19.1) | 49.78 (20.1) | 49.19 (20.3) | 25.75 (38.8) |
| Dynamic reentry, one level | 89.33 (11.2) | 86.83 (11.5) | 79.53 (12.6) | 137.79 (7.3) |
| Typed reentry, two levels | 96.45 (10.4) | 91.51 (10.9) | 92.50 (10.8) | 40.55 (24.7) |
| Dynamic reentry, two levels | 159.66 (6.3) | 154.16 (6.5) | 141.99 (7.0) | 218.34 (4.6) |
| Native Rust add control | 1.83 (545.6) | 1.83 (545.5) | 1.86 (537.9) | — |

## Wasm guest → Rust host

Each typed outer entry runs 4,096 dependent guest iterations. Every import iteration calls the host once. A pending import suspends and resumes once per iteration. The guest-only control keeps the same arithmetic and byte load in the guest. The raw slope is divided by 4,096.

| Case | Base | Imports | Scalar | Wasmtime |
| --- | ---: | ---: | ---: | ---: |
| Guest-only arithmetic | 1.78 (562.9) | 1.81 (552.4) | 1.81 (551.7) | 1.88 (530.5) |
| Typed sync | 15.34 (65.2) | 13.56 (73.8) | 13.44 (74.4) | 3.58 (279.2) |
| Dynamic sync | 72.89 (13.7) | 53.87 (18.6) | 53.18 (18.8) | 31.36 (31.9) |
| Typed ready | 148.10 (6.8) | 58.56 (17.1) | 54.12 (18.5) | 48.93 (20.4) |
| Dynamic ready | 97.03 (10.3) | 83.70 (11.9) | 84.36 (11.9) | 72.02 (13.9) |
| Typed suspended once | 213.47 (4.7) | 165.88 (6.0) | 156.60 (6.4) | 163.55 (6.1) |
| Dynamic suspended once | 165.02 (6.1) | 148.25 (6.7) | 139.98 (7.1) | 195.10 (5.1) |

## Paired changes

Each cell shows the median within-block latency change followed by the one-sided 95% upper bound. Negative numbers mean faster calls. The CSV contains all 57 Rust comparisons when both suites are present.

| Case | Imports vs Base | Scalar vs Imports |
| --- | ---: | ---: |
| Export · Typed | +1.4% / +3.2% (1/8 faster) | -1.0% / +1.2% (6/8 faster) |
| Export · Dynamic | -0.5% / +4.1% (4/8 faster) | -9.6% / -8.5% (8/8 faster) |
| Export · Typed ready | +1.3% / +4.0% (1/8 faster) | -0.6% / +0.3% (6/8 faster) |
| Export · Dynamic ready | +1.7% / +3.4% (1/8 faster) | -3.4% / +2.2% (6/8 faster) |
| Export · Typed suspended once | -8.1% / -5.9% (8/8 faster) | +1.2% / +1.8% (0/8 faster) |
| Export · Dynamic suspended once | -8.0% / -5.4% (8/8 faster) | +0.3% / +2.0% (4/8 faster) |
| Export · Guest → host add, including outer call | -8.0% / -6.6% (8/8 faster) | +1.5% / +3.4% (0/8 faster) |
| Export · Typed reentry, one level | -4.5% / -1.8% (8/8 faster) | -0.5% / +0.4% (4/8 faster) |
| Export · Dynamic reentry, one level | -3.4% / +0.9% (5/8 faster) | -9.4% / -6.1% (8/8 faster) |
| Export · Typed reentry, two levels | -5.0% / -2.9% (8/8 faster) | +2.9% / +3.9% (3/8 faster) |
| Export · Dynamic reentry, two levels | -2.5% / +0.1% (6/8 faster) | -7.9% / -4.7% (8/8 faster) |
| Import · Typed sync | -15.5% / -7.1% (8/8 faster) | +2.0% / +7.7% (2/8 faster) |
| Import · Dynamic sync | -23.0% / -17.7% (8/8 faster) | +4.8% / +11.2% (3/8 faster) |
| Import · Typed ready | -62.7% / -59.9% (8/8 faster) | +0.2% / +6.5% (4/8 faster) |
| Import · Dynamic ready | -14.0% / -8.9% (8/8 faster) | +0.8% / +2.7% (3/8 faster) |
| Import · Typed suspended once | -22.7% / -17.9% (8/8 faster) | -3.3% / +5.7% (5/8 faster) |
| Import · Dynamic suspended once | -11.2% / -3.7% (7/8 faster) | -1.3% / +7.8% (4/8 faster) |

The Rust verification recalculated 594 Criterion point estimates from 14,256 samples, checked per-case call IDs and fixture hashes, verified every crossover position and adjacent pair, and recomputed all 57 paired comparisons. The raw block table includes batch size, unnormalized slope and ns per operation. [Rust block estimates](rust-blocks.csv) · [Paired results](rust-paired.csv)

A superseded 138-estimate pilot overlapped a compiler using 971% aggregate CPU. The primary run restarted from block one after a bounded check for compiler activity (under 100% aggregate over three checks, five seconds apart, or two minutes elapsed). All eight primary blocks remain in the results. Criterion analysis used one Rayon worker.
At restart, tracked compilers were using 0.0% CPU after 61.0 seconds of observation.

Across the 594 retained processes, median one-minute system load was 8.7 (range 4.7–36.1). Median whole-process CPU time was 101.9% of elapsed time (tenth percentile 91.8%). The process diagnostic sums all threads and includes calibration, warm-up and analysis, so it can exceed 100%. Per-call Rust results use elapsed time. [Allocation counts, requests and bytes per operation](rust-allocations.csv)

The entry suite had median system load 6.8 and median whole-process CPU/elapsed 102.3% (10th–90th percentile 101.5–102.9%).

The imports suite had median system load 24.7 and median whole-process CPU/elapsed 94.9% (10th–90th percentile 89.3–99.6%).
