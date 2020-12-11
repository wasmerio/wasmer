#![allow(dead_code)]

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

#[derive(WasmerEnv)]
struct MyUnitStruct;

#[derive(WasmerEnv)]
struct MyTupleStruct(u32);

#[derive(WasmerEnv)]
struct MyTupleStruct2(u32, u32);

#[derive(WasmerEnv)]
struct MyTupleStructWithAttribute(#[wasmer(export(name = "memory"))] LazyInit<Memory>, u32);

#[test]
fn test_derive_with_attribute() {
    assert!(impls_wasmer_env::<MyEnvWithMemory>());
    assert!(impls_wasmer_env::<MyEnvWithFuncs>());
    assert!(impls_wasmer_env::<MyEnvWithEverything>());
    assert!(impls_wasmer_env::<MyEnvWithLifetime>());
    assert!(impls_wasmer_env::<MyUnitStruct>());
    assert!(impls_wasmer_env::<MyTupleStruct>());
    assert!(impls_wasmer_env::<MyTupleStruct2>());
    assert!(impls_wasmer_env::<MyTupleStructWithAttribute>());
}
