use super::{async_test, common::Exit};
use anyhow::Result;
use futures::channel::oneshot;
use std::{cell::RefCell, rc::Rc};
use wasmer::{Function, Instance, Module, Store, Value, imports};

#[async_test]
async fn suspended_calls_resume_independently() -> Result<()> {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
            (import "" "wait" (func $wait (param i32) (result i32)))
            (func (export "run") (param i32) (result i32)
                local.get 0 i32.const 1 i32.eq
                if (result i32)
                    i32.const 1 call $wait i32.const 100 i32.add
                else
                    i32.const 2 call $wait i32.const 200 i32.add
                end))"#,
    )?;
    let (start_a, entered_a) = oneshot::channel();
    let (start_b, entered_b) = oneshot::channel();
    let (release_a, answer_a) = oneshot::channel::<i32>();
    let (release_b, answer_b) = oneshot::channel::<i32>();
    let requests = RefCell::new([Some((start_a, answer_a)), Some((start_b, answer_b))]);
    let wait = Function::new_typed_async(&mut store, move |i: i32| {
        let (started, answer) = requests.borrow_mut()[(i - 1) as usize].take().unwrap();
        async move {
            started.send(()).unwrap();
            i + answer.await.unwrap()
        }
    });
    let instance = Instance::new(&mut store, &module, &imports! { "" => { "wait" => wait } })?;
    let run = instance.exports.get_function("run")?;
    let store = store.into_async();
    let first = run.call_async(&store, vec![Value::I32(1)]);
    let second = async {
        let result = run.call_async(&store, vec![Value::I32(2)]).await?;
        release_a.send(10).unwrap();
        Ok(result)
    };
    let release_second = async {
        entered_a.await.unwrap();
        entered_b.await.unwrap();
        release_b.send(20).unwrap();
        Ok(())
    };
    let (a, b, ()) = futures::try_join!(first, second, release_second)?;
    assert_eq!(a[0].unwrap_i32(), 111);
    assert_eq!(b[0].unwrap_i32(), 222);
    Ok(())
}

#[async_test]
async fn host_error_reaches_guest_caller_after_suspension() -> Result<()> {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
        (import "" "fail" (func $fail))
        (func (export "run") call $fail))"#,
    )?;
    let (start, entered) = oneshot::channel();
    let (release, resume) = oneshot::channel();
    let request = RefCell::new(Some((start, resume)));
    let fail = Function::new_typed_async(&mut store, move || {
        let (start, resume) = request.borrow_mut().take().unwrap();
        async move {
            start.send(()).unwrap();
            resume.await.unwrap();
            Err::<(), _>(Exit(23))
        }
    });
    let instance = Instance::new(&mut store, &module, &imports! { "" => { "fail" => fail } })?;
    let run = instance
        .exports
        .get_typed_function::<(), ()>(&store, "run")?;
    let store = store.into_async();
    let invoke = async {
        let error = run.call_async(&store).await.unwrap_err();
        assert_eq!(
            error.downcast_ref::<Exit>().map(|exit| exit.0),
            Some(23),
            "{error}"
        );
        Ok::<(), anyhow::Error>(())
    };
    let complete = async {
        entered.await?;
        release.send(()).unwrap();
        Ok(())
    };
    futures::try_join!(invoke, complete)?;
    Ok(())
}

#[async_test]
async fn ready_import_can_be_called_through_a_guest_export_or_reexport() -> Result<()> {
    let mut store = Store::default();
    let module = Module::new(
        &store,
        r#"(module
        (import "" "ready" (func $ready (result i32)))
        (export "reexport" (func $ready))
        (func (export "run") (result i32) call $ready))"#,
    )?;
    let calls = Rc::new(std::cell::Cell::new(0));
    let counter = calls.clone();
    let ready = Function::new_typed_async(&mut store, move || {
        counter.set(counter.get() + 1);
        std::future::ready(42i32)
    });
    let instance = Instance::new(
        &mut store,
        &module,
        &imports! { "" => { "ready" => ready } },
    )?;
    let run = instance
        .exports
        .get_typed_function::<(), i32>(&store, "run")?;
    let reexport = instance
        .exports
        .get_typed_function::<(), i32>(&store, "reexport")?;
    let store = store.into_async();
    assert_eq!(run.call_async(&store).await?, 42);
    assert_eq!(reexport.call_async(&store).await?, 42);
    assert_eq!(calls.get(), 2);
    Ok(())
}
