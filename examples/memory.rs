use wasmer::{
    imports, wat2wasm, Extern, Function, Instance, Memory, MemoryType, Module, NativeFunc, Pages,
    Store, Table, TableType, Type, Value,
};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_jit::JIT;

// this example is a work in progress:
// TODO: clean it up and comment it

fn main() -> anyhow::Result<()> {
    let wasm_bytes = wat2wasm(
        r#"
(module
  (type $mem_size_t (func (result i32)))
  (type $get_at_t (func (param i32) (result i32)))
  (type $set_at_t (func (param i32) (param i32)))

  (memory $mem 1)

  (func $get_at (type $get_at_t) (param $idx i32) (result i32)
    (i32.load (local.get $idx)))

  (func $set_at (type $set_at_t) (param $idx i32) (param $val i32)
    (i32.store (local.get $idx) (local.get $val)))

  (func $mem_size (type $mem_size_t) (result i32)
    (memory.size))

  (export "get_at" (func $get_at))
  (export "set_at" (func $set_at))
  (export "mem_size" (func $mem_size))
  (export "memory" (memory $mem)))
"#
        .as_bytes(),
    )?;

    // We set up our store with an engine and a compiler.
    let store = Store::new(&JIT::new(&Cranelift::default()).engine());
    // Then compile our Wasm.
    let module = Module::new(&store, wasm_bytes)?;
    let import_object = imports! {};
    // And instantiate it with no imports.
    let instance = Instance::new(&module, &import_object)?;

    let mem_size: NativeFunc<(), i32> = instance.exports.get_native_function("mem_size")?;
    let get_at: NativeFunc<i32, i32> = instance.exports.get_native_function("get_at")?;
    let set_at: NativeFunc<(i32, i32), ()> = instance.exports.get_native_function("set_at")?;
    let memory = instance.exports.get_memory("memory")?;

    let mem_addr = 0x2220;
    let val = 0xFEFEFFE;

    assert_eq!(memory.size(), Pages::from(1));
    memory.grow(2)?;
    assert_eq!(memory.size(), Pages::from(3));
    let result = mem_size.call()?;
    assert_eq!(result, 3);

    // -------------
    set_at.call(mem_addr, val)?;
    // -------------

    let page_size = 0x1_0000;
    let result = get_at.call(page_size * 3 - 4)?;
    memory.grow(1025)?;
    assert_eq!(memory.size(), Pages::from(1028));
    set_at.call(page_size * 1027 - 4, 123456)?;
    let result = get_at.call(page_size * 1027 - 4)?;
    assert_eq!(result, 123456);
    set_at.call(1024, 123456)?;
    let result = get_at.call(1024)?;
    assert_eq!(result, 123456);

    // -------------
    let result = get_at.call(mem_addr)?;
    assert_eq!(result, val);
    // -------------

    Ok(())
}
