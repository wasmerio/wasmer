//! Micro-bench for the host -> wasm typed-call latency (the per-call cost
//! of crossing the trampoline + running a one-instruction Singlepass body).
//!
//! Run from the wasmer root:
//!
//! ```shell
//! cargo bench --bench typed_call --features "singlepass"
//! ```
//!
//! This is the same methodology that produced the per-call numbers cited in
//! the `tls-stack-cache` PR description (Intel i9-10940X, eIBRS):
//!
//!   * Singlepass-compiled wasm body is `i32.const 1; i32.const 1; i32.add`
//!     so virtually all of the measured time is `host -> wasm trampoline +
//!     return`; the wasm-internal body is one x86 `ADD`.
//!   * The `TypedFunction<(), i32>` handle is fetched ONCE outside the
//!     timer so the export-name lookup is amortized away.
//!   * `black_box` on the return value keeps LLVM from DCE'ing the call.
//!   * Criterion handles warmup, sample count, and outlier reporting.
//!
//! Compare wasmer main vs the patched fork with the same command on the
//! same hardware — criterion will print the delta directly.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use wasmer::{Instance, Module, Store, imports, wat2wasm};
use wasmer_compiler_singlepass::Singlepass;

const WAT: &str = r#"
(module
    (func (export "lit") (result i32)
        i32.const 1
        i32.const 1
        i32.add)
    (func (export "add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add))
"#;

fn fresh_singlepass_instance() -> (Store, Instance) {
    let wasm = wat2wasm(WAT.as_bytes()).expect("wat parse");
    let compiler = Singlepass::default();
    let mut store = Store::new(compiler);
    let module = Module::new(&store, wasm).expect("singlepass compile");
    let instance = Instance::new(&mut store, &module, &imports! {}).expect("instantiate");
    (store, instance)
}

/// `lit` takes no args, returns 2 from a 3-instruction wasm body. Closest
/// thing to a "pure trampoline cost" measurement; the arg/result marshaling
/// done by `TypedFunction` is the only thing on top of the trampoline.
fn bench_typed_call_lit(c: &mut Criterion) {
    let (mut store, instance) = fresh_singlepass_instance();
    let lit = instance
        .exports
        .get_typed_function::<(), i32>(&store, "lit")
        .expect("export `lit`");

    c.bench_function("typed_call/lit_one_plus_one", |b| {
        b.iter(|| {
            let r = lit.call(&mut store).unwrap();
            black_box(r);
        });
    });
}

/// `add` takes two i32 args. Same wasm body cost as `lit`; the difference
/// vs `lit` measures the cost of marshaling two i32 args through the
/// TypedFunction trampoline.
fn bench_typed_call_add(c: &mut Criterion) {
    let (mut store, instance) = fresh_singlepass_instance();
    let add = instance
        .exports
        .get_typed_function::<(i32, i32), i32>(&store, "add")
        .expect("export `add`");

    c.bench_function("typed_call/add_one_plus_one", |b| {
        b.iter(|| {
            let r = add.call(&mut store, black_box(1), black_box(1)).unwrap();
            black_box(r);
        });
    });
}

/// Sanity baseline: the same `1 + 1` in native Rust with `black_box`
/// on the inputs. Anchors the wasm numbers — if the typed-call benches
/// somehow regressed to this floor, that would mean LLVM had folded the
/// wasm body away (which it hasn't; the typed-call numbers stay ~30x
/// higher even after the TLS-stack-cache patch).
fn bench_native_baseline(c: &mut Criterion) {
    c.bench_function("native_baseline/one_plus_one", |b| {
        b.iter(|| {
            let r = black_box(1i32) + black_box(1i32);
            black_box(r);
        });
    });
}

criterion_group!(
    benches,
    bench_typed_call_lit,
    bench_typed_call_add,
    bench_native_baseline,
);
criterion_main!(benches);
