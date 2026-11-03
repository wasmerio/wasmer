use macro_wasmer_engine_test::engine_test;
#[cfg(feature = "js")]
use wasm_bindgen_test::wasm_bindgen_test;

use wasmer::*;

#[engine_test]
fn data_and_store_mut() -> Result<(), String> {
    let mut store = Store::default();
    let global_mut = Global::new_mut(&mut store, Value::I32(10));
    struct Env {
        value: i32,
        global: Global,
    }
    let env = FunctionEnv::new(
        &mut store,
        Env {
            value: 0i32,
            global: global_mut,
        },
    );
    let mut envmut = env.into_mut(&mut store);

    let (data, mut storemut) = envmut.data_and_store_mut();

    assert_eq!(
        data.global.ty(&storemut),
        GlobalType {
            ty: Type::I32,
            mutability: Mutability::Var
        }
    );
    assert_eq!(data.global.get(&mut storemut), Value::I32(10));
    data.value = data.global.get(&mut storemut).unwrap_i32() + 10;

    data.global
        .set(&mut storemut, Value::I32(data.value))
        .unwrap();

    assert_eq!(data.global.get(&mut storemut), Value::I32(data.value));

    Ok(())
}

#[test]
#[cfg(all(feature = "sys", feature = "experimental-async"))]
fn weak_async_environment_does_not_retain_its_store() {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    struct State(Arc<AtomicUsize>);
    impl Drop for State {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, State(drops.clone()));
    // The Store owns this callback, which in turn owns its weak environment.
    let weak_slot = Rc::new(RefCell::new(None));
    let owned_slot = weak_slot.clone();
    let function = Function::new_with_env_async(
        &mut store,
        &env,
        FunctionType::new([], []),
        move |env, _| {
            *owned_slot.borrow_mut() = Some(env.downgrade().unwrap());
            async { Ok(vec![]) }
        },
    );
    let store = store.into_async();
    futures::executor::block_on(function.call_async(&store, vec![])).unwrap();
    let weak = weak_slot.borrow().as_ref().unwrap().clone();
    assert!(weak.upgrade().is_some());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(store);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(weak.upgrade().is_none());
    assert!(weak_slot.borrow().as_ref().unwrap().upgrade().is_none());
}
