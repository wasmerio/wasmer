# V8 calls on Apple M5 Max

The public JS-to-Wasm addition took **3.63 ns** with a separate export wrapper and **1.64 ns** with the wrapper integrated into the JS driver. Both paths called a separately compiled Wasm body. One genuine reentry took **10.14 ns** and **6.05 ns** respectively. All numbers below are medians from this local native arm64 run.

## Synchronous entry and reentry

Each cell is benchmark-thread CPU ns per outer call, including the dependent caller loop and guest work. The nested fixtures use identity with one i32 parameter. Host callback counts confirmed both reentry depths.

| Case | Separate export wrapper | Integrated export wrapper |
| --- | ---: | ---: |
| JS addition control | 0.27 | 0.27 |
| Wasm addition | 3.63 | 1.64 |
| Wasm identity | 3.53 | 1.63 |
| Wasm → JS identity import | 6.59 | 4.09 |
| Wasm → JS → Wasm | 10.14 | 6.05 |
| Wasm → JS → Wasm → JS → Wasm | 13.28 | 11.70 |

The separate mode disabled both wrapper and body inlining. In integrated-wrapper mode the JS driver directly loads and calls an indirect Wasm target. Disassembly of the separate-mode driver shows dispatch to a JS_TO_WASM_FUNCTION instead. Separate arm64 code listings captured the TurboFan guest and the remaining call in both modes. All 256 timed export samples held optimization status 41 before and after the measured loop.

## Async JSPI entry

The promising exports and prefulfilled Promise were created before timing. Async cases awaited each call before starting the next. The pending import allocated a Promise and resolved it in a queued microtask. A separately gated continuation remained suspended before release and ran the post-import Wasm instruction only after release.

| Case | Thread CPU ns/call | Wall ns/call |
| --- | ---: | ---: |
| Ordinary synchronous Wasm add | 3.74 | 3.77 |
| Await the ordinary Wasm scalar | 19.12 | 19.35 |
| Await a promising Wasm add with no import | 184.00 | 192.32 |
| Await promising Wasm with a plain import | 183.66 | 189.64 |
| Suspending import returns a scalar | 262.70 | 268.82 |
| Suspending import returns the prefulfilled Promise | 256.11 | 264.01 |
| Suspending import resolves from a new microtask | 293.88 | 301.74 |
| Pure JS async control resolving from a new microtask | 79.64 | 80.44 |

Paired across seven blocks, returning a newly pending Promise from the suspending import cost **40.31 ns** on top of returning a prefulfilled Promise. This includes Promise allocation, microtask scheduling and continuation work.

## Guest-driven imports

Both native Rust runners and V8 used identical loop and import module bytes. Each export performed 4,096 dependent steps before returning. The unit is a guest-loop step. The promising export is entered and awaited once per batch, while a suspending import may suspend on every step.

| Case | Thread CPU ns/step | Wall ns/step |
| --- | ---: | ---: |
| Guest arithmetic loop | 1.53 | 1.55 |
| Guest calls a synchronous JS import | 4.18 | 4.22 |
| Promising export, guest arithmetic loop | 1.62 | 1.64 |
| Promising export, guest calls a plain import | 6.35 | 6.43 |
| Suspending import returns a scalar | 49.06 | 49.85 |
| Suspending import resolves from a microtask | 85.59 | 86.36 |

## Reproduction and coverage

Node v26.0.0, V8 14.6.202.33-node.19, macOS Darwin 27.0.0 arm64, Apple M5 Max. The three suites ran sequentially before the Rust timed phase, with Rust release compilation held during this window. On macOS the OS selected cores and frequencies. Darwin CLOCK_THREAD_CPUTIME_ID had a reported resolution of 42 ns and counted only 0.099 ms during a 42.51 ms blocking preflight. Per-thread values cover the JS benchmark thread. The wall durations also include scheduling delays.

The exact fixture revision was 509ba1e0a25ea00e5f2ef5c676a0a45d635627d5 from native study 44197172290. Each of the 394 timed observations ran in a fresh Node process. The synchronous suite had eight shuffled blocks, JSPI seven rotated blocks, and guest-driven imports nine shuffled blocks. The export check counted callbacks and all three suites asserted checksums. The detailed evidence records the generated JSPI module, exact source hashes, compiler flags, native arm64 disassembly and the explicit suspension check. [All per-block CPU and wall samples](samples.csv) · [Fixtures, source and verification](source-and-verification.zip)
