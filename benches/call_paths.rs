//! What a synchronous call costs on each path between the host and the
//! guest, with Cranelift: calls into the guest (`TypedFunction::call` and
//! `Function::call`), and calls from the guest to host functions (typed,
//! with an environment, dynamic, and one that calls back into the guest).
//!
//! Run from the wasmer root:
//!
//! ```shell
//! cargo bench --bench call_paths --features cranelift
//! ```
//!
//! Every case reports the time of one call. A call from the guest is timed
//! inside a guest loop that makes as many calls as Criterion asks for, each
//! one feeding the next, so the loop's own cost (about a nanosecond per
//! iteration) is included and the calls cannot be batched.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, Store, Type,
    TypedFunction, Value, imports,
};

const WAT: &str = r#"
(module
  (import "h" "typed"     (func $typed     (param i32) (result i32)))
  (import "h" "typed_env" (func $typed_env (param i32) (result i32)))
  (import "h" "dyn"       (func $dyn       (param i32) (result i32)))
  (import "h" "dyn_env"   (func $dyn_env   (param i32) (result i32)))
  (import "h" "reenter"   (func $reenter   (param i32) (result i32)))

  (func (export "noop"))
  (func (export "const42") (result i32) (i32.const 42))
  (func (export "add") (param i32 i32) (result i32)
    (i32.add (local.get 0) (local.get 1)))
  (func (export "inc") (param i32) (result i32)
    (i32.add (local.get 0) (i32.const 1)))

  (func (export "loop_typed") (param $n i32) (result i32) (local $acc i32)
    (block $done (loop $l
      (br_if $done (i32.eqz (local.get $n)))
      (local.set $acc (call $typed (local.get $acc)))
      (local.set $n (i32.sub (local.get $n) (i32.const 1)))
      (br $l)))
    (local.get $acc))
  (func (export "loop_typed_env") (param $n i32) (result i32) (local $acc i32)
    (block $done (loop $l
      (br_if $done (i32.eqz (local.get $n)))
      (local.set $acc (call $typed_env (local.get $acc)))
      (local.set $n (i32.sub (local.get $n) (i32.const 1)))
      (br $l)))
    (local.get $acc))
  (func (export "loop_dyn") (param $n i32) (result i32) (local $acc i32)
    (block $done (loop $l
      (br_if $done (i32.eqz (local.get $n)))
      (local.set $acc (call $dyn (local.get $acc)))
      (local.set $n (i32.sub (local.get $n) (i32.const 1)))
      (br $l)))
    (local.get $acc))
  (func (export "loop_dyn_env") (param $n i32) (result i32) (local $acc i32)
    (block $done (loop $l
      (br_if $done (i32.eqz (local.get $n)))
      (local.set $acc (call $dyn_env (local.get $acc)))
      (local.set $n (i32.sub (local.get $n) (i32.const 1)))
      (br $l)))
    (local.get $acc))
  (func (export "loop_reenter") (param $n i32) (result i32) (local $acc i32)
    (block $done (loop $l
      (br_if $done (i32.eqz (local.get $n)))
      (local.set $acc (call $reenter (local.get $acc)))
      (local.set $n (i32.sub (local.get $n) (i32.const 1)))
      (br $l)))
    (local.get $acc)))
"#;

struct Env {
    delta: i32,
    inc: Option<TypedFunction<i32, i32>>,
}

struct Guest {
    store: Store,
    instance: Instance,
}

fn guest() -> Guest {
    let mut store = Store::default();
    let module = Module::new(&store, WAT).unwrap();
    let env = FunctionEnv::new(
        &mut store,
        Env {
            delta: 1,
            inc: None,
        },
    );
    let typed = Function::new_typed(&mut store, |x: i32| x.wrapping_add(1));
    let typed_env =
        Function::new_typed_with_env(&mut store, &env, |env: FunctionEnvMut<Env>, x: i32| {
            x.wrapping_add(env.data().delta)
        });
    let ty = FunctionType::new([Type::I32], [Type::I32]);
    let dynamic = Function::new(&mut store, ty.clone(), |args: &[Value]| {
        Ok(vec![Value::I32(args[0].unwrap_i32().wrapping_add(1))])
    });
    let dynamic_env = Function::new_with_env(
        &mut store,
        &env,
        ty,
        |env: FunctionEnvMut<Env>, args: &[Value]| {
            Ok(vec![Value::I32(
                args[0].unwrap_i32().wrapping_add(env.data().delta),
            )])
        },
    );
    let reenter =
        Function::new_typed_with_env(&mut store, &env, |mut env: FunctionEnvMut<Env>, x: i32| {
            let (data, mut store) = env.data_and_store_mut();
            data.inc.as_ref().unwrap().call(&mut store, x).unwrap()
        });
    let imports = imports! {
        "h" => {
            "typed" => typed,
            "typed_env" => typed_env,
            "dyn" => dynamic,
            "dyn_env" => dynamic_env,
            "reenter" => reenter,
        }
    };
    let instance = Instance::new(&mut store, &module, &imports).unwrap();
    let inc = instance.exports.get_typed_function(&store, "inc").unwrap();
    env.as_mut(&mut store).inc = Some(inc);
    Guest { store, instance }
}

fn calls_into_the_guest(c: &mut Criterion) {
    let Guest {
        mut store,
        instance,
    } = guest();
    let exports = &instance.exports;
    let const42: TypedFunction<(), i32> = exports.get_typed_function(&store, "const42").unwrap();
    let add: TypedFunction<(i32, i32), i32> = exports.get_typed_function(&store, "add").unwrap();
    let add_dynamic = exports.get_function("add").unwrap().clone();
    let noop_dynamic = exports.get_function("noop").unwrap().clone();

    let mut group = c.benchmark_group("call_paths/into_guest");
    group.bench_function("typed_call_no_args", |b| {
        b.iter(|| black_box(const42.call(&mut store).unwrap()))
    });
    group.bench_function("typed_call_add", |b| {
        b.iter(|| black_box(add.call(&mut store, black_box(1), 2).unwrap()))
    });
    let params = [Value::I32(1), Value::I32(2)];
    group.bench_function("function_call_add", |b| {
        b.iter(|| {
            black_box(
                add_dynamic
                    .call(&mut store, black_box(&params[..]))
                    .unwrap(),
            )
        })
    });
    group.bench_function("function_call_noop", |b| {
        b.iter(|| black_box(noop_dynamic.call(&mut store, &[]).unwrap()))
    });
    group.finish();
}

/// Times `iters` calls of a host function, made by the guest loop `function`.
fn guest_loop(store: &mut Store, function: &TypedFunction<i32, i32>, iters: u64) -> Duration {
    let start = Instant::now();
    let mut left = iters;
    while left > 0 {
        let n = left.min(i32::MAX as u64);
        black_box(function.call(store, black_box(n as i32)).unwrap());
        left -= n;
    }
    start.elapsed()
}

fn calls_from_the_guest(c: &mut Criterion) {
    let Guest {
        mut store,
        instance,
    } = guest();
    let mut group = c.benchmark_group("call_paths/from_guest");
    for (case, export) in [
        ("import_typed", "loop_typed"),
        ("import_typed_with_env", "loop_typed_env"),
        ("import_dynamic", "loop_dyn"),
        ("import_dynamic_with_env", "loop_dyn_env"),
        ("import_typed_calling_back_into_the_guest", "loop_reenter"),
    ] {
        let function: TypedFunction<i32, i32> =
            instance.exports.get_typed_function(&store, export).unwrap();
        assert_eq!(function.call(&mut store, 1000).unwrap(), 1000);
        group.bench_function(case, |b| {
            b.iter_custom(|iters| guest_loop(&mut store, &function, iters))
        });
    }
    group.finish();
}

criterion_group!(benches, calls_into_the_guest, calls_from_the_guest);
criterion_main!(benches);
