# Native Apple M5 Max call comparison

The Rust and V8 studies ran locally on one Apple M5 Max with 18 cores. The V8 timing completed before compiling and measuring Rust. All executables used native arm64 instructions. macOS scheduled the processes.

- [Rust timings, throughput, revisions and paired changes](rust-report.md)
- [Rust per-block elapsed times](rust-blocks.csv) and [paired effects](rust-paired.csv), plus [allocation counts](rust-allocations.csv)
- [V8 results, elapsed and thread-CPU samples, generated code and sources](v8/README.md)

The Rust host-to-Wasm study measures one public API call. The Wasm-to-host study divides each batch by 4,096 guest-loop iterations. Each imported callback performs the same scalar add. Ready and genuinely suspended imports have separate rows. Nested export measurements include the complete callback and reentry chain. The fixed dynamic scalar-result variant is d3b2953ef5 on top of the published imports-only implementation acb8542694. The base c2366fe3c7 predates the import changes.

Rust used eight complete balanced four-way blocks and a six-block three-way native control. Fractions of faster paired blocks and one-sided 95% upper bounds apply to each case individually. The complete Criterion run contains 14,256 samples. The source fixtures and external dependency lock match [the four-way AMD64 comparison](https://github.com/wasmerio/wasmer/tree/509ba1e0a25ea00e5f2ef5c676a0a45d635627d5/benches/x64_calls).

To reproduce the Rust runs, unpack the [local benchmark driver and verification script](arm64-rust-driver.zip), with Rust 1.99.1, Python 3.11+, and LLVM 23.1.5 available. Start from a scratch clone of Wasmer with the c2366fe3c7 submodules checked out, verify the three study commits are present, and put the included run_m5.py alongside the immutable entry.rs, imports.rs, alloc_counts.rs, Benchmark.lock and v8/imports/ bytecode. Set LLVM_SYS_231_PREFIX to the installed LLVM directory. A fresh Cargo cache may require removing the build command's --offline option to fetch the locked crates.

Run the two suites with the following command. The build verifies the fixture bytecode, recompiles the Wasmer VM, API and LLVM backend for each revision, runs correctness probes and allocation counts, and saves architecture and binary digests before timing.

```sh
python3 harness/run_m5.py --repo ./wasmer --output ./study/raw \
  --imports-only acb8542694cd3970d96e39088b8873dfe8cc627c \
  --candidate d3b2953ef5354ecc55a04e6f1ed58df3aba28798
```

The verifier expects harness/ and raw/ below one study directory. Run `python3 report.py study output` to reproduce both Rust tables and both CSVs from the complete raw artifact. Each line of rust-blocks.csv identifies the suite, case, arm, block, crossover position, unnormalized Criterion slope in nanoseconds, iterations per batch, normalized elapsed ns per operation, whole-process CPU and elapsed seconds, and system load. Whole-process CPU sums all threads and includes setup, calibration and analysis. For each arm the reported value is the median of the per-block values. For each pair it is the median of within-block ratios.
