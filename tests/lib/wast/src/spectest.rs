use wasmer::*;

/// Return an instance implementing the "spectest" interface used in the
/// spec testsuite.
#[allow(clippy::print_stdout)]
pub fn spectest_importobject(store: &mut Store) -> Imports {
    let print = Function::new_typed(store, || {});
    let print_i32 = Function::new_typed(store, |val: i32| println!("{val}: i32"));
    let print_i64 = Function::new_typed(store, |val: i64| println!("{val}: i64"));
    let print_f32 = Function::new_typed(store, |val: f32| println!("{val}: f32"));
    let print_f64 = Function::new_typed(store, |val: f64| println!("{val}: f64"));
    let print_i32_f32 = Function::new_typed(store, |i: i32, f: f32| {
        println!("{i}: i32");
        println!("{f}: f32");
    });
    let print_f64_f64 = Function::new_typed(store, |f1: f64, f2: f64| {
        println!("{f1}: f64");
        println!("{f2}: f64");
    });

    let global_i32 = Global::new(store, Value::I32(666));
    let global_i64 = Global::new(store, Value::I64(666));
    let global_f32 = Global::new(store, Value::F32(666.6));
    let global_f64 = Global::new(store, Value::F64(666.6));

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
