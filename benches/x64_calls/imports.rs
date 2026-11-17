//! One guest entry drives BATCH import calls. Criterion reports time per batch.
use criterion::{Criterion, criterion_group, criterion_main};
use std::{
    cell::Cell,
    future::Future,
    hint::black_box,
    task::{Context, Poll, Waker},
};
use wasmer::{Function, FunctionType, Instance, Module, Store, Type, Value, imports};

const BATCH: i32 = 4096;
const SEED: i32 = 23;
const CASES: [&str; 7] = [
    "guest_loop",
    "import_typed",
    "import_dynamic",
    "import_typed_ready",
    "import_dynamic_ready",
    "import_typed_suspend",
    "import_dynamic_suspend",
];

thread_local! { static CALLS: Cell<u32> = const { Cell::new(0) }; }

fn host_add<const COUNT: bool>(a: i32, b: i32) -> i32 {
    if COUNT {
        CALLS.set(CALLS.get() + 1);
    }
    a.wrapping_add(b)
}
fn byte(index: usize) -> u8 {
    ((index * 13 + 17) % 251 + 1) as u8
}
fn expected(count: i32, mut value: i32) -> i32 {
    for _ in 0..count {
        value = value.wrapping_add(byte((value & 63) as usize) as i32);
    }
    value
}
fn wasm(imported: bool) -> Vec<u8> {
    let data = (0..64)
        .map(|i| format!("\\{:02x}", byte(i)))
        .collect::<String>();
    let operation = if imported { "call $step" } else { "i32.add" };
    wasmer::wat2wasm(
        format!(
            r#"(module
        (import "h" "step" (func $step (param i32 i32) (result i32)))
        (memory 1) (data (i32.const 0) "{data}")
        (func (export "run") (param $count i32) (param $acc i32) (result i32)
          (block $done (loop $again
            local.get $count i32.eqz br_if $done
            local.get $acc local.get $acc i32.const 63 i32.and i32.load8_u
            {operation} local.set $acc
            local.get $count i32.const 1 i32.sub local.set $count br $again))
          local.get $acc))"#
        )
        .as_bytes(),
    )
    .unwrap()
    .into_owned()
}
async fn host_async<const COUNT: bool, const SUSPEND: bool>(a: i32, b: i32) -> i32 {
    if SUSPEND {
        let mut yielded = false;
        std::future::poll_fn(|cx| {
            if yielded {
                return Poll::Ready(());
            }
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await;
    }
    host_add::<COUNT>(a, b)
}
fn finish<F: Future>(future: F) -> (F::Output, i32) {
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    let mut suspensions = 0;
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(r) => return (r, suspensions),
            Poll::Pending => suspensions += 1,
        }
    }
}
fn bench<const COUNT: bool>(
    c: &mut Criterion,
    engine: &str,
    index: usize,
    mut call: impl FnMut(i32, i32) -> (i32, i32),
) {
    for (n, seed) in [(0, 0), (1, 41), (17, -16), (BATCH, SEED)] {
        if COUNT {
            CALLS.set(0);
        }
        let (out, susp) = call(n, seed);
        assert_eq!(out, expected(n, seed));
        assert_eq!(susp, if index >= 5 { n } else { 0 });
        if COUNT {
            assert_eq!(CALLS.get(), if index == 0 { 0 } else { n as u32 });
        }
    }
    c.bench_function(&format!("{engine}/{}", CASES[index]), |b| {
        b.iter(|| black_box(call(black_box(BATCH), black_box(SEED)).0))
    });
}
fn dynamic_async<const COUNT: bool, const SUSPEND: bool>(
    store: &mut Store,
    ty: &FunctionType,
) -> Function {
    Function::new_async(store, ty.clone(), |args| {
        let (a, b) = (args[0].unwrap_i32(), args[1].unwrap_i32());
        async move { Ok(vec![Value::I32(host_async::<COUNT, SUSPEND>(a, b).await)]) }
    })
}
fn wasmer_cases<const COUNT: bool>(c: &mut Criterion) {
    let mut store = Store::new(wasmer_compiler_llvm::LLVM::new());
    let local = Module::new(&store, wasm(false)).unwrap();
    let imported = Module::new(&store, wasm(true)).unwrap();
    let ty = FunctionType::new([Type::I32, Type::I32], [Type::I32]);
    let typed = Function::new_typed(&mut store, host_add::<COUNT>);
    let dynamic = Function::new(&mut store, ty.clone(), |args| {
        Ok(vec![Value::I32(host_add::<COUNT>(
            args[0].unwrap_i32(),
            args[1].unwrap_i32(),
        ))])
    });
    let callbacks = [
        typed.clone(),
        typed,
        dynamic,
        Function::new_typed_async(&mut store, host_async::<COUNT, false>),
        dynamic_async::<COUNT, false>(&mut store, &ty),
        Function::new_typed_async(&mut store, host_async::<COUNT, true>),
        dynamic_async::<COUNT, true>(&mut store, &ty),
    ];
    let runs = callbacks
        .into_iter()
        .enumerate()
        .map(|(i, cb)| {
            let module = if i == 0 { &local } else { &imported };
            Instance::new(&mut store, module, &imports! {"h"=>{"step"=>cb}})
                .unwrap()
                .exports
                .get_typed_function::<(i32, i32), i32>(&store, "run")
                .unwrap()
        })
        .collect::<Vec<_>>();
    for (i, run) in runs[..3].iter().enumerate() {
        bench::<COUNT>(c, "llvm", i, |n, seed| {
            (run.call(&mut store, n, seed).unwrap(), 0)
        });
    }
    let store = store.into_async();
    for (i, run) in runs[3..].iter().enumerate() {
        bench::<COUNT>(c, "llvm", i + 3, |n, seed| {
            let (out, susp) = finish(run.call_async(&store, n, seed));
            (out.unwrap(), susp)
        });
    }
}
fn wt_dynamic<const COUNT: bool, const SUSPEND: bool>(
    store: &mut wasmtime::Store<()>,
    ty: &wasmtime::FuncType,
) -> wasmtime::Func {
    wasmtime::Func::new_async(store, ty.clone(), |_, args, out| {
        Box::new(async move {
            out[0] = wasmtime::Val::I32(
                host_async::<COUNT, SUSPEND>(args[0].unwrap_i32(), args[1].unwrap_i32()).await,
            );
            Ok(())
        })
    })
}
fn wasmtime_cases<const COUNT: bool>(c: &mut Criterion) {
    use wasmtime::{
        Config, Engine, Func, FuncType, Instance, Module, Store, Strategy, Val, ValType,
    };
    let mut cfg = Config::new();
    cfg.strategy(Strategy::Cranelift);
    let engine = Engine::new(&cfg).unwrap();
    let local = Module::new(&engine, wasm(false)).unwrap();
    let imported = Module::new(&engine, wasm(true)).unwrap();
    let ty = FuncType::new(&engine, [ValType::I32, ValType::I32], [ValType::I32]);
    for i in 0..CASES.len() {
        let mut store = Store::new(&engine, ());
        let cb = match i {
            0 | 1 => Func::wrap(&mut store, host_add::<COUNT>),
            2 => Func::new(&mut store, ty.clone(), |_, a, r| {
                r[0] = Val::I32(host_add::<COUNT>(a[0].unwrap_i32(), a[1].unwrap_i32()));
                Ok(())
            }),
            3 => Func::wrap_async(&mut store, |_, (a, b)| {
                Box::new(host_async::<COUNT, false>(a, b))
            }),
            4 => wt_dynamic::<COUNT, false>(&mut store, &ty),
            5 => Func::wrap_async(&mut store, |_, (a, b)| {
                Box::new(host_async::<COUNT, true>(a, b))
            }),
            6 => wt_dynamic::<COUNT, true>(&mut store, &ty),
            _ => unreachable!(),
        };
        let module = if i == 0 { &local } else { &imported };
        let instance = if i < 3 {
            Instance::new(&mut store, module, &[cb.into()]).unwrap()
        } else {
            finish(Instance::new_async(&mut store, module, &[cb.into()]))
                .0
                .unwrap()
        };
        let run = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "run")
            .unwrap();
        if i < 3 {
            bench::<COUNT>(c, "wasmtime", i, |n, seed| {
                (run.call(&mut store, (n, seed)).unwrap(), 0)
            });
        } else {
            bench::<COUNT>(c, "wasmtime", i, |n, seed| {
                let (out, susp) = finish(run.call_async(&mut store, (n, seed)));
                (out.unwrap(), susp)
            });
        }
    }
}
fn study(c: &mut Criterion) {
    if std::env::var_os("CALL_STUDY_VERIFY").is_some() {
        wasmer_cases::<true>(c);
        wasmtime_cases::<true>(c);
    } else {
        wasmer_cases::<false>(c);
        wasmtime_cases::<false>(c);
    }
}
criterion_group!(benches, study);
criterion_main!(benches);
