use anyhow::Result;
use wasmer::*;

#[test]
fn exports_work_after_multiple_instances_have_been_freed() -> Result<()> {
    let store = Store::default();
    let module = Module::new(
        &store,
        "
    (module
      (type $sum_t (func (param i32 i32) (result i32)))
      (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
        local.get $x
        local.get $y
        i32.add)
      (export \"sum\" (func $sum_f)))
",
    )?;

    let import_object = ImportObject::new();
    let instance = Instance::new(&module, &import_object)?;
    let instance2 = instance.clone();
    let instance3 = instance.clone();

    // The function is cloned to “break” the connection with `instance`.
    let sum = instance.exports.get_function("sum")?.clone();

    drop(instance);
    drop(instance2);
    drop(instance3);

    // All instances have been dropped, but `sum` continues to work!
    assert_eq!(
        sum.call(&[Value::I32(1), Value::I32(2)])?.into_vec(),
        vec![Value::I32(3)],
    );

    Ok(())
}

#[test]
fn instance_local_memory_lifetime() -> Result<()> {
    let store = Store::default();

    /*
        let wat1 = r#"(module
        (memory $mem 1)
        (export "memory" (memory $mem))
    )"#;
        let module1 = Module::new(&store, wat1)?;
        let instance1 = Instance::new(&module1, &imports! {})?;
        let memory = instance1.exports.get_memory("memory")?.clone();
        */

    let memory: Memory = {
        let wat = r#"(module
    (memory $mem 1)
    (export "memory" (memory $mem))
)"#;
        let module = Module::new(&store, wat)?;
        let instance = Instance::new(&module, &imports! {})?;
        instance.exports.get_memory("memory")?.clone()
    };

    let wat = r#"(module
    (import "env" "memory" (memory $mem 1) )
    (func $get_at (type $get_at_t) (param $idx i32) (result i32)
      (i32.load (local.get $idx)))

    (type $get_at_t (func (param i32) (result i32)))
    (type $set_at_t (func (param i32) (param i32)))
    (func $set_at (type $set_at_t) (param $idx i32) (param $val i32)
      (i32.store (local.get $idx) (local.get $val)))
    (export "get_at" (func $get_at))
    (export "set_at" (func $set_at))
)"#;
    let module = Module::new(&store, wat)?;
    let imports = imports! {
        "env" => {
            "memory" => memory,
        },
    };
    let instance = Instance::new(&module, &imports)?;
    let set_at: NativeFunc<(i32, i32), ()> = instance.exports.get_native_function("set_at")?;
    let get_at: NativeFunc<i32, i32> = instance.exports.get_native_function("get_at")?;
    set_at.call(200, 123)?;
    assert_eq!(get_at.call(200)?, 123);

    Ok(())
}
