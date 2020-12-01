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
