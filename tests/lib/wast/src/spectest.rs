use wasmer::*;

/// Return an instance implementing the "spectest" interface used in the
/// spec testsuite.
pub fn spectest_importobject(store: &mut Store) -> Imports {
    let print = Function::new_typed(store, || {});
    let print_i32 = Function::new_typed(store, |val: i32| println!("{}: i32", val));
    let print_i64 = Function::new_typed(store, |val: i64| println!("{}: i64", val));
    let print_f32 = Function::new_typed(store, |val: f32| println!("{}: f32", val));
    let print_f64 = Function::new_typed(store, |val: f64| println!("{}: f64", val));
    let print_i32_f32 = Function::new_typed(store, |i: i32, f: f32| {
        println!("{}: i32", i);
        println!("{}: f32", f);
    });
    let print_f64_f64 = Function::new_typed(store, |f1: f64, f2: f64| {
        println!("{}: f64", f1);
        println!("{}: f64", f2);
    });

    let global_i32 = Global::new(store, Value::I32(666));
    let global_i64 = Global::new(store, Value::I64(666));
    let global_f32 = Global::new(store, Value::F32(f32::from_bits(0x4426_8000)));
    let global_f64 = Global::new(store, Value::F64(f64::from_bits(0x4084_d000_0000_0000)));

    let ty = TableType::new(Type::FuncRef, 10, Some(20));
    let table = Table::new(store, ty, Value::FuncRef(None)).unwrap();

    let ty = MemoryType::new(1, Some(2), false);
    let memory = Memory::new(store, ty).unwrap();

    let ty = MemoryType::new(1, Some(2), true);
    let shared_memory = Memory::new(store, ty).unwrap();

    imports! {
        "spectest" => {
            "print" => print,
            "print_i32" => print_i32,
            "print_i64" => print_i64,
            "print_f32" => print_f32,
            "print_f64" => print_f64,
            "print_i32_f32" => print_i32_f32,
            "print_f64_f64" => print_f64_f64,
            "global_i32" => global_i32,
            "global_i64" => global_i64,
            "global_f32" => global_f32,
            "global_f64" => global_f64,
            "table" => table,
            "memory" => memory,
            "shared_memory" => shared_memory,
        },
    }
}
