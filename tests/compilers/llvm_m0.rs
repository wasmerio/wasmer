use anyhow::Result;
use std::{fs, path::Path};
use wasmer::{
    Function, FunctionType, Instance, MemoryStyle, MemoryType, Module, Store, Type, Value, imports,
    sys::{BaseTunables, CompilerConfig, EngineBuilder, Tunables},
};
use wasmer_compiler_llvm::{LLVM, LLVMCallbacks};
use wasmer_types::target::UserCompilerOptimizations;

fn read_preopt_ir(path: &Path) -> Result<Vec<String>> {
    let mut modules = Vec::new();
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_dir() {
            modules.extend(read_preopt_ir(&path)?);
        } else if path.to_string_lossy().ends_with(".preopt.ll") {
            modules.push(fs::read_to_string(path)?);
        }
    }
    Ok(modules)
}

fn static_memory_calls(enable_m0: bool) -> Result<()> {
    let temp = tempfile::tempdir()?;
    let mut config = LLVM::new();
    config.enable_verifier();
    config.callbacks(Some(LLVMCallbacks::new(temp.path().to_owned())?));
    let tunables = BaseTunables::new();
    assert!(matches!(
        tunables.memory_style(&MemoryType::new(1, Some(2), false)),
        MemoryStyle::Static
    ));
    let mut engine = EngineBuilder::new(config).engine();
    engine.with_opts(&UserCompilerOptimizations {
        pass_params: Some(enable_m0),
    })?;
    engine.set_tunables(tunables);
    let mut store = Store::new(engine);
    let module = Module::new(
        &store,
        r#"(module
            (type $unary (func (param i32) (result i32)))
            (import "host" "typed" (func $typed (type $unary)))
            (import "host" "dynamic" (func $dynamic (type $unary)))
            (memory (export "memory") 1 2)
            (table (export "table") 4 funcref)
            (elem (i32.const 0) $load $typed $dynamic)
            (data (i32.const 0) "\2a\00\00\00")
            (func $load (export "load") (type $unary)
                (i32.load (local.get 0)))
            (func (export "direct") (type $unary)
                (call $load (local.get 0)))
            (func (export "indirect") (param i32 i32) (result i32)
                (call_indirect (type $unary) (local.get 0) (local.get 1)))
            (func (export "host_calls") (type $unary)
                (i32.add (call $typed (local.get 0)) (call $dynamic (local.get 0))))
            (func (export "multi") (param i32) (result i32 i64 i32)
                (call $load (local.get 0))
                (i64.const 123456789)
                (i32.const 7))
            (func (export "grow") (result i32)
                (memory.grow (i32.const 1))))"#,
    )?;

    // Check the lowered ABI before executing it: functions and call trampolines
    // must agree, and disabling m0 must retain static memory code generation.
    let ir = read_preopt_ir(temp.path())?;
    assert!(!ir.is_empty());
    assert_eq!(ir.iter().any(|ir| ir.contains("%m0_base_ptr")), enable_m0);
    assert_eq!(
        ir.iter().any(|ir| ir.contains("%trmpl_m0_base_ptr")),
        enable_m0
    );
    assert!(ir.iter().all(|ir| !ir.contains("load_offset_end")));

    let typed = Function::new_typed(&mut store, |value: i32| value + 10);
    let dynamic = Function::new(
        &mut store,
        FunctionType::new([Type::I32], [Type::I32]),
        |args| Ok(vec![Value::I32(args[0].unwrap_i32() + 20)]),
    );
    let imports = imports! { "host" => { "typed" => typed, "dynamic" => dynamic } };
    // Exercise the serialized artifact too: its trampolines must retain this ABI.
    let serialized = module.serialize()?;
    let module = unsafe { Module::deserialize(&store, serialized)? };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let direct = instance
        .exports
        .get_typed_function::<i32, i32>(&store, "direct")?;
    assert_eq!(direct.call(&mut store, 0)?, 42);
    let indirect = instance
        .exports
        .get_typed_function::<(i32, i32), i32>(&store, "indirect")?;
    assert_eq!(indirect.call(&mut store, 0, 0)?, 42);
    assert_eq!(indirect.call(&mut store, 7, 1)?, 17);
    assert_eq!(indirect.call(&mut store, 7, 2)?, 27);
    assert_eq!(
        &*instance
            .exports
            .get_function("host_calls")?
            .call(&mut store, &[Value::I32(7)])?,
        &[Value::I32(44)]
    );
    assert_eq!(
        &*instance
            .exports
            .get_function("multi")?
            .call(&mut store, &[Value::I32(0)])?,
        &[Value::I32(42), Value::I64(123456789), Value::I32(7)]
    );
    let memory = instance.exports.get_memory("memory")?;
    let base = memory.view(&store).data_ptr();
    let grow = instance
        .exports
        .get_typed_function::<(), i32>(&store, "grow")?;
    assert_eq!(grow.call(&mut store)?, 1);
    assert_eq!(memory.view(&store).data_ptr(), base);
    memory.view(&store).write(65536, &99_i32.to_le_bytes())?;
    assert_eq!(direct.call(&mut store, 65536)?, 99);
    assert_eq!(indirect.call(&mut store, 65536, 0)?, 99);
    assert!(direct.call(&mut store, 131072).is_err());
    assert_eq!(grow.call(&mut store)?, -1);

    if !enable_m0 {
        // A runtime-populated table entry must not be dispatched using the m0
        // optimization's assumptions about the original element initializer.
        let load = instance.exports.get_function("load")?.clone();
        instance
            .exports
            .get_table("table")?
            .set(&mut store, 3, Value::FuncRef(Some(load)))?;
        assert_eq!(indirect.call(&mut store, 65536, 3)?, 99);
    }
    Ok(())
}

#[test]
fn static_memory_without_m0() -> Result<()> {
    static_memory_calls(false)
}

#[test]
fn static_memory_with_m0() -> Result<()> {
    static_memory_calls(true)
}
