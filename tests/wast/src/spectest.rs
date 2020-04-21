use wasmer::import::ImportObject;
use wasmer::types::ElementType;
use wasmer::units::Pages;
use wasmer::wasm::{Func, Global, Memory, MemoryType, Table, TableType, Value};
use wasmer::*;

/// Return an instance implementing the "spectest" interface used in the
/// spec testsuite.
pub fn spectest_importobject() -> ImportObject {
    let print = Func::new(|| {});
    let print_i32 = Func::new(|val: i32| println!("{}: i32", val));
    let print_i64 = Func::new(|val: i64| println!("{}: i64", val));
    let print_f32 = Func::new(|val: f32| println!("{}: f32", val));
    let print_f64 = Func::new(|val: f64| println!("{}: f64", val));
    let print_i32_f32 = Func::new(|i: i32, f: f32| {
        println!("{}: i32", i);
        println!("{}: f32", f);
    });
    let print_f64_f64 = Func::new(|f1: f64, f2: f64| {
        println!("{}: f64", f1);
        println!("{}: f64", f2);
    });

    let global_i32 = Global::new(Value::I32(666));
    let global_i64 = Global::new(Value::I64(666));
    let global_f32 = Global::new(Value::F32(f32::from_bits(0x4426_8000)));
    let global_f64 = Global::new(Value::F64(f64::from_bits(0x4084_d000_0000_0000)));

    let memory_desc = MemoryType::new(Pages(1), Some(Pages(2)), false).unwrap();
    let memory = Memory::new(memory_desc).unwrap();

    let table = Table::new(TableType {
        element: ElementType::Anyfunc,
        minimum: 10,
        maximum: Some(20),
    })
    .unwrap();

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
        },
    }
}
