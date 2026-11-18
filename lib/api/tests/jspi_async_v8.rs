#![cfg(all(
    feature = "experimental-async",
    feature = "v8",
    not(target_arch = "wasm32")
))]

use anyhow::Result;
use futures::future;
use std::cell::RefCell;
use wasmer::{
    Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module, Store, Type,
    TypedFunction, Value, imports,
};
use wasmer_types::TrapCode;

#[test]
fn v8_suspended_calls_keep_their_own_continuations() -> Result<()> {
    use futures::{channel::oneshot, poll};
    use std::{collections::VecDeque, rc::Rc};

    let mut store = Store::new(wasmer::v8::V8::new());
    assert!(store.engine().supports_async());
    let module = Module::new(
        &store,
        r#"
        (module
          (import "" "wait" (func $wait (param i32) (result i32)))
          (func (export "run") (param i32) (result i32 i32)
            local.get 0 i32x4.splat
            local.get 0 call $wait i32.const 1 i32.add local.set 0
            i32x4.extract_lane 3 local.get 0))
    "#,
    )?;
    let receivers = Rc::new(RefCell::new(VecDeque::new()));
    let queue = receivers.clone();
    let wait = Function::new_typed_async(&mut store, move |input: i32| {
        let receiver: oneshot::Receiver<i32> = queue.borrow_mut().pop_front().unwrap();
        async move { input + receiver.await.unwrap() }
    });
    let instance = Instance::new(&mut store, &module, &imports! { "" => { "wait" => wait } })?;
    let run = instance.exports.get_function("run")?;
    let store = store.into_async();
    futures::executor::block_on(async {
        let (cancel, first) = oneshot::channel();
        let (release, second) = oneshot::channel();
        receivers.borrow_mut().extend([first, second]);
        let mut a = run.call_async(&store, vec![Value::I32(1)]);
        let mut b = run.call_async(&store, vec![Value::I32(2)]);
        assert!(poll!(&mut a).is_pending());
        assert!(poll!(&mut b).is_pending());
        drop(a);
        assert!(cancel.send(10).is_err());
        release.send(20).unwrap();
        let result = b.await?;
        assert_eq!(result[0].unwrap_i32(), 2);
        assert_eq!(result[1].unwrap_i32(), 23);
        Ok(())
    })
}

#[test]
fn v8_suspending_import_round_trips_funcref() -> Result<()> {
    let mut store = Store::new(wasmer::v8::V8::new());
    let module = Module::new(
        &store,
        r#"(module
        (import "" "echo" (func $echo (param funcref) (result funcref)))
        (func (export "run") (param funcref) (result funcref)
          local.get 0 call $echo)
        (func (export "answer") (result i32) i32.const 42))"#,
    )?;
    let ty = FunctionType::new([Type::FuncRef], [Type::FuncRef]);
    let echo = Function::new_async(&mut store, ty, |values| {
        let values = values.to_vec();
        async move { Ok(values) }
    });
    let instance = Instance::new(&mut store, &module, &imports! { "" => { "echo" => echo } })?;
    let run = instance.exports.get_function("run")?;
    let answer = instance.exports.get_function("answer")?.clone();
    let store = store.into_async();
    let results =
        futures::executor::block_on(run.call_async(&store, vec![Value::FuncRef(Some(answer))]))?;
    let Some(reference) = results[0].unwrap_funcref() else {
        panic!("expected a function reference")
    };
    let result = futures::executor::block_on(reference.call_async(&store, Vec::new()))?;
    assert_eq!(result[0].unwrap_i32(), 42);
    Ok(())
}

#[test]
fn v8_rejects_v128_at_async_boundaries() -> Result<()> {
    let mut store = Store::new(wasmer::v8::V8::new());
    let module = Module::new(
        &store,
        r#"(module
        (import "" "consume" (func $consume (param v128)))
        (import "" "produce" (func $produce (result v128)))
        (func (export "call_consume") v128.const i32x4 1 2 3 4 call $consume)
        (func (export "call_produce") call $produce drop)
        (func (export "param") (param v128))
        (func (export "result") (result v128) v128.const i32x4 1 2 3 4))"#,
    )?;
    let consume = Function::new_async(&mut store, FunctionType::new([Type::V128], []), |_| async {
        Ok(vec![])
    });
    let produce = Function::new_async(&mut store, FunctionType::new([], [Type::V128]), |_| async {
        Ok(vec![Value::V128(0)])
    });
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "" => { "consume" => consume, "produce" => produce } },
    )?;
    let store = store.into_async();
    for (name, args) in [
        ("call_consume", vec![]),
        ("call_produce", vec![]),
        ("param", vec![Value::V128(0)]),
        ("result", vec![]),
    ] {
        let function = instance.exports.get_function(name)?;
        let error = futures::executor::block_on(function.call_async(&store, args)).unwrap_err();
        assert!(
            error.message().contains("v128 parameters or results"),
            "{name}: {error}"
        );
    }
    Ok(())
}

#[test]
fn v8_preserves_host_errors_after_suspension() -> Result<()> {
    let mut store = Store::new(wasmer::v8::V8::new());
    let fail = Function::new_async(&mut store, FunctionType::new([], []), |_| async {
        tokio::task::yield_now().await;
        Err(wasmer::RuntimeError::new("failed after suspension"))
    });
    let store = store.into_async();
    let error = futures::executor::block_on(fail.call_async(&store, vec![])).unwrap_err();
    assert_eq!(error.message(), "failed after suspension");
    Ok(())
}

#[test]
fn v8_ready_typed_import_and_foreign_store() -> Result<()> {
    let mut store = Store::new(wasmer::v8::V8::new());
    let add = Function::new_typed_async(&mut store, |n: i32| future::ready(n + 1));
    let typed = add.typed::<i32, i32>(&store)?;
    assert_eq!(typed.call(&mut store, 6)?, 7);
    drop(store);

    let other = Store::new(wasmer::v8::V8::new()).into_async();
    assert!(futures::executor::block_on(typed.call_async(&other, 6)).is_err());
    assert!(futures::executor::block_on(add.call_async(&other, vec![Value::I32(6)])).is_err());
    Ok(())
}

#[test]
fn v8_synchronous_guest_call_traps_before_ready_import() -> Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut store = Store::new(wasmer::v8::V8::new());
    let module = Module::new(
        &store,
        r#"(module
        (import "" "ready" (func $ready (result i32)))
        (export "reexport" (func $ready))
        (func (export "run") (result i32) call $ready))"#,
    )?;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let ready = Function::new_typed_async(&mut store, move || {
        counter.fetch_add(1, Ordering::Relaxed);
        future::ready(42i32)
    });
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "" => { "ready" => ready } },
    )?;
    let functions = [
        instance
            .exports
            .get_typed_function::<(), i32>(&store, "run")?,
        instance
            .exports
            .get_typed_function::<(), i32>(&store, "reexport")?,
    ];
    for func in &functions {
        assert_eq!(
            func.call(&mut store).unwrap_err().to_trap(),
            Some(TrapCode::YieldOutsideAsyncContext)
        );
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    let store = store.into_async();
    for func in &functions {
        assert_eq!(futures::executor::block_on(func.call_async(&store))?, 42);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    Ok(())
}

#[test]
fn v8_sync_callback_can_call_ready_typed_import() -> Result<()> {
    let mut store = Store::new(wasmer::v8::V8::new());
    let add = Function::new_typed_async(&mut store, |n: i32| future::ready(n + 1));
    let inner = add.typed::<i32, i32>(&store)?;
    let env = FunctionEnv::new(&mut store, inner);
    let callback = Function::new_typed_with_env(
        &mut store,
        &env,
        |mut env: FunctionEnvMut<TypedFunction<i32, i32>>, n: i32| {
            let (inner, mut store) = env.data_and_store_mut();
            inner.call(&mut store, n)
        },
    );
    let entry = callback.typed::<i32, i32>(&store)?;
    let result = futures::executor::block_on(entry.call_async(&store.into_async(), 41))?;
    assert_eq!(result, 42);
    Ok(())
}

#[test]
fn v8_funcrefs_keep_their_store_after_synchronous_reads() -> Result<()> {
    let mut store = Store::new(wasmer::v8::V8::new());
    let module = Module::new(
        &store,
        r#"(module
        (func $answer (result i32) i32.const 42)
        (table (export "table") 1 funcref)
        (elem (i32.const 0) $answer)
        (global (export "global") funcref (ref.func $answer))
        (func (export "get") (result funcref) ref.func $answer))"#,
    )?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let values = [
        instance
            .exports
            .get_function("get")?
            .call(&mut store, &[])?[0]
            .clone(),
        instance
            .exports
            .get_table("table")?
            .get(&mut store, 0)
            .unwrap(),
        instance.exports.get_global("global")?.get(&mut store),
    ];
    let store = store.into_async();
    for value in &values {
        let func = value.unwrap_funcref().as_ref().unwrap();
        let result = futures::executor::block_on(func.call_async(&store, vec![]))?;
        assert_eq!(result[0].unwrap_i32(), 42);
    }
    drop(store);
    let other = Store::new(wasmer::v8::V8::new()).into_async();
    for value in &values {
        let func = value.unwrap_funcref().as_ref().unwrap();
        assert!(futures::executor::block_on(func.call_async(&other, vec![])).is_err());
    }
    Ok(())
}
