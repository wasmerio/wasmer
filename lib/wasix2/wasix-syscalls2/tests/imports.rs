//! Instantiates guests against [`imports`] backed by the all-defaults [`Wasix2Env`].

use wasix_syscalls2::{Wasix2Env, imports};
use wasix_types2::{Errno, SYSCALLS, ValType};
use wasmer::{Extern, FunctionEnv, Instance, Module, Store, Type, TypedFunction};

const GUEST: &str = r#"
(module
  (import "wasix2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
  (import "wasix2" "pread64" (func $pread64 (param i32 i32 i32 i64) (result i32)))
  (import "wasix2" "exit_group" (func $exit_group (param i32)))
  (import "wasix_ext" "thread_spawn" (func $thread_spawn (param i32) (result i32)))
  (func (export "openat") (result i32)
    (call $openat (i32.const -100) (i32.const 0x1000) (i32.const 0x80000000) (i32.const 420)))
  (func (export "pread64") (result i32)
    (call $pread64 (i32.const 3) (i32.const 0x2000) (i32.const 16) (i64.const -1)))
  (func (export "thread_spawn") (result i32)
    (call $thread_spawn (i32.const 0x3000)))
  (func (export "exit_group")
    (call $exit_group (i32.const 1))))
"#;

#[test]
fn defaults_return_enosys() {
    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, Wasix2Env::new());
    let imports = imports(&mut store, &env);
    let module = Module::new(&store, GUEST).unwrap();
    let instance = Instance::new(&mut store, &module, &imports).unwrap();

    for name in ["openat", "pread64", "thread_spawn"] {
        let f: TypedFunction<(), i32> = instance.exports.get_typed_function(&store, name).unwrap();
        let ret = f.call(&mut store).unwrap();
        assert_eq!(ret, -38, "{name}");
        assert_eq!(Errno::from_ret(ret), Some(Errno::ENOSYS));
    }

    let exit: TypedFunction<(), ()> = instance
        .exports
        .get_typed_function(&store, "exit_group")
        .unwrap();
    let err = exit.call(&mut store).unwrap_err();
    assert!(err.message().contains("exit_group"), "{err}");
}

fn wasm_type(v: ValType) -> Type {
    match v {
        ValType::I32 => Type::I32,
        ValType::I64 => Type::I64,
    }
}

/// The generated host functions must have exactly the signatures `SYSCALLS` declares.
#[test]
fn imports_match_syscall_metadata() {
    let mut store = Store::default();
    let env = FunctionEnv::new(&mut store, Wasix2Env::new());
    let imports = imports(&mut store, &env);
    for def in SYSCALLS {
        let Some(Extern::Function(f)) = imports.get_export(def.module, def.import_name) else {
            panic!("missing import {}.{}", def.module, def.import_name);
        };
        let ty = f.ty(&store);
        let params: Vec<Type> = def.params.iter().map(|p| wasm_type(p.ty)).collect();
        let results: Vec<Type> = def.result.into_iter().map(wasm_type).collect();
        assert_eq!(ty.params(), params, "{}.{}", def.module, def.import_name);
        assert_eq!(ty.results(), results, "{}.{}", def.module, def.import_name);
    }
    assert_eq!(imports.into_iter().count(), SYSCALLS.len());
}

#[test]
fn errno_ret_roundtrip() {
    assert_eq!(Errno::ENOENT.to_ret(), -2);
    assert_eq!(Errno::from_ret(-2), Some(Errno::ENOENT));
    assert_eq!(Errno::from_ret(0), None);
    assert_eq!(Errno::from_ret(-4096), None);
    assert_eq!(Errno::from_ret(i32::MIN), None);
}
