use wasmer::{Function, Global, LazyInit, Memory, NativeFunc, Table, WasmerEnv};

#[derive(WasmerEnv)]
struct MyEnv {
    num: u32,
    nums: Vec<i32>,
}

fn impls_wasmer_env<T: WasmerEnv>() -> bool {
    true
}

#[test]
fn test_derive() {
    let _my_env = MyEnv {
        num: 3,
        nums: vec![1, 2, 3],
    };
    assert!(impls_wasmer_env::<MyEnv>());
}

#[derive(WasmerEnv)]
struct MyEnvWithMemory {
    num: u32,
    nums: Vec<i32>,
    #[wasmer(export)]
    memory: LazyInit<Memory>,
}

#[derive(WasmerEnv)]
struct MyEnvWithFuncs {
    num: u32,
    nums: Vec<i32>,
    #[wasmer(export)]
    memory: LazyInit<Memory>,
    #[wasmer(export)]
    sum: LazyInit<NativeFunc<(i32, i32), i32>>,
}

#[derive(WasmerEnv)]
struct MyEnvWithEverything {
    num: u32,
    nums: Vec<i32>,
    #[wasmer(export)]
    memory: LazyInit<Memory>,
    #[wasmer(export)]
    sum: LazyInit<NativeFunc<(), i32>>,
    #[wasmer(export)]
    multiply: LazyInit<Function>,
    #[wasmer(export)]
    counter: LazyInit<Global>,
    #[wasmer(export)]
    functions: LazyInit<Table>,
}

#[derive(WasmerEnv)]
struct MyEnvWithLifetime<'a> {
    name: &'a str,
    #[wasmer(export(name = "memory"))]
    memory: LazyInit<Memory>,
}

#[test]
fn test_derive_with_attribute() {
    //use wasmer::WasmerEnv;
    assert!(impls_wasmer_env::<MyEnvWithMemory>());
    assert!(impls_wasmer_env::<MyEnvWithFuncs>());
    assert!(impls_wasmer_env::<MyEnvWithEverything>());
    assert!(impls_wasmer_env::<MyEnvWithLifetime>());
}
