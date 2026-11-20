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

/// An environment replaced through the VM's objects with a value of another
/// type is refused by a `FunctionEnv` of the old type, instead of being read
/// as one.
#[cfg(all(
    feature = "sys",
    not(feature = "v8-default"),
    not(target_arch = "wasm32")
))]
#[test]
fn a_replaced_environment_is_refused() {
    use wasmer::sys::vm::{StoreObject, VMFunctionEnvironment};

    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, 7u32);
    assert_eq!(*env.as_ref(&store), 7);
    // Safety: replaces function environments, which nothing points into.
    for slot in unsafe { VMFunctionEnvironment::list_mut(store.objects_mut().as_sys_mut()) } {
        *slot = VMFunctionEnvironment::new(1u8);
    }
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| *env.as_ref(&store)));
    assert!(read.is_err());
}
