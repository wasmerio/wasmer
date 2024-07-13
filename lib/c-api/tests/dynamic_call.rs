use wasmer::wasm_c_api::{
    externals::{wasm_func_new, wasm_func_new_impl},
    store::wasm_store_t,
    trap::wasm_trap_t,
    value::{wasm_val_t, wasm_val_vec_t},
};
use wasmer_api::{AsStoreMut, Function, FunctionType, Store};
use wasmer_compiler::wasmparser::ValType;
use wasmer_types::Type;

unsafe extern "C" fn host_func_callback(
    args: &wasm_val_vec_t,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>> {
    println!("hello host_func_callback");
    // let s = results.as_slice_mut();
    // s[0] = wasm_val_t::from(42);

    None
}

#[test]
fn dynamic_call() {
    // (import "env" "memory" (memory 17))
    let code = r#"
(module
  (type (func))
  (import "env" "func" (func $imported_func (type 0)))
  (func $other_func (type 0))
  (func $func (type 0)
    i32.const 1
    call_indirect (type 0)
  )
  (table 3 3 funcref)
  (export "func" (func $func))
  (elem (i32.const 1) func $imported_func $other_func)
) "#;

    let engine = wasmer_api::Engine::default();
    let module = wasmer_api::Module::new(&engine, code).unwrap();
    let mut store = wasmer_api::Store::new(engine.clone());

    let mem = wasmer_api::Memory::new(
        &mut store,
        wasmer_api::MemoryType::new(wasmer_api::Pages(17), None, false),
    )
    .unwrap();

    let ty = FunctionType::new(vec![], vec![]);
    let func = wasm_func_new_impl(store.as_store_mut(), &ty, host_func_callback);

    // let func2 = wasmer_api::Function::new_typed(&mut store, || {});
    // let func = func2;

    let imports = wasmer_api::imports! {
        "env" => {
            "func" => func,
            // "memory" => mem,
        }
    };

    let instance = wasmer_api::Instance::new(&mut store, &module, &imports).unwrap();

    let func = instance.exports.get_function("func").unwrap();
    let out = func.call(&mut store, &[]).unwrap();
    dbg!(out);
}
