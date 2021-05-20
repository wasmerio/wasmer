use anyhow::Result;
use wasmer::*;

const MEM_WAT: &str = "
    (module
      (func $host_fn (import \"env\" \"host_fn\") (param) (result))
      (func (export \"call_host_fn\") (param) (result)
          (call $host_fn))

      (memory $mem 0)
      (export \"memory\" (memory $mem))
      )
";

const GLOBAL_WAT: &str = "
    (module
      (func $host_fn (import \"env\" \"host_fn\") (param) (result))
      (func (export \"call_host_fn\") (param) (result)
          (call $host_fn))

      (global $global i32 (i32.const 11))
      (export \"global\" (global $global))
      )
";

const TABLE_WAT: &str = "
    (module
      (func $host_fn (import \"env\" \"host_fn\") (param) (result))
      (func (export \"call_host_fn\") (param) (result)
          (call $host_fn))

      (table $table 4 4 funcref)
      (export \"table\" (table $table))
      )
";

const FUNCTION_WAT: &str = "
    (module
      (func $host_fn (import \"env\" \"host_fn\") (param) (result))
      (func (export \"call_host_fn\") (param) (result)
          (call $host_fn))
      )
";

#[test]
fn strong_weak_behavior_works_memory() -> Result<()> {
    #[derive(Clone, Debug, WasmerEnv, Default)]
    struct MemEnv {
        #[wasmer(export)]
        memory: LazyInit<Memory>,
    }

    let host_fn = |env: &MemEnv| {
        let mem = env.memory_ref().unwrap();
        assert_eq!(mem.is_strong_instance_ref(), Some(false));
        let mem_clone = mem.clone();
        assert_eq!(mem_clone.is_strong_instance_ref(), Some(true));
        assert_eq!(mem.is_strong_instance_ref(), Some(false));
    };

    let f: NativeFunc<(), ()> = {
        let store = Store::default();
        let module = Module::new(&store, MEM_WAT)?;
        let env = MemEnv::default();

        let instance = Instance::new(
            &module,
            &imports! {
                "env" => {
                    "host_fn" => Function::new_native_with_env(&store, env, host_fn)
                }
            },
        )?;

        {
            let mem = instance.exports.get_memory("memory")?;
            assert_eq!(mem.is_strong_instance_ref(), Some(true));
        }

        let f: NativeFunc<(), ()> = instance.exports.get_native_function("call_host_fn")?;
        f.call()?;
        f
    };
    f.call()?;

    Ok(())
}

#[test]
fn strong_weak_behavior_works_global() -> Result<()> {
    #[derive(Clone, Debug, WasmerEnv, Default)]
    struct GlobalEnv {
        #[wasmer(export)]
        global: LazyInit<Global>,
    }

    let host_fn = |env: &GlobalEnv| {
        let global = env.global_ref().unwrap();
        assert_eq!(global.is_strong_instance_ref(), Some(false));
        let global_clone = global.clone();
        assert_eq!(global_clone.is_strong_instance_ref(), Some(true));
        assert_eq!(global.is_strong_instance_ref(), Some(false));
    };

    let f: NativeFunc<(), ()> = {
        let store = Store::default();
        let module = Module::new(&store, GLOBAL_WAT)?;
        let env = GlobalEnv::default();

        let instance = Instance::new(
            &module,
            &imports! {
                "env" => {
                    "host_fn" => Function::new_native_with_env(&store, env, host_fn)
                }
            },
        )?;

        {
            let global = instance.exports.get_global("global")?;
            assert_eq!(global.is_strong_instance_ref(), Some(true));
        }

        let f: NativeFunc<(), ()> = instance.exports.get_native_function("call_host_fn")?;
        f.call()?;
        f
    };
    f.call()?;

    Ok(())
}

#[test]
fn strong_weak_behavior_works_table() -> Result<()> {
    #[derive(Clone, WasmerEnv, Default)]
    struct TableEnv {
        #[wasmer(export)]
        table: LazyInit<Table>,
    }

    let host_fn = |env: &TableEnv| {
        let table = env.table_ref().unwrap();
        assert_eq!(table.is_strong_instance_ref(), Some(false));
        let table_clone = table.clone();
        assert_eq!(table_clone.is_strong_instance_ref(), Some(true));
        assert_eq!(table.is_strong_instance_ref(), Some(false));
    };

    let f: NativeFunc<(), ()> = {
        let store = Store::default();
        let module = Module::new(&store, TABLE_WAT)?;
        let env = TableEnv::default();

        let instance = Instance::new(
            &module,
            &imports! {
                "env" => {
                    "host_fn" => Function::new_native_with_env(&store, env, host_fn)
                }
            },
        )?;

        {
            let table = instance.exports.get_table("table")?;
            assert_eq!(table.is_strong_instance_ref(), Some(true));
        }

        let f: NativeFunc<(), ()> = instance.exports.get_native_function("call_host_fn")?;
        f.call()?;
        f
    };
    f.call()?;

    Ok(())
}

#[test]
fn strong_weak_behavior_works_function() -> Result<()> {
    #[derive(Clone, WasmerEnv, Default)]
    struct FunctionEnv {
        #[wasmer(export)]
        call_host_fn: LazyInit<Function>,
    }

    let host_fn = |env: &FunctionEnv| {
        let function = env.call_host_fn_ref().unwrap();
        assert_eq!(function.is_strong_instance_ref(), Some(false));
        let function_clone = function.clone();
        assert_eq!(function_clone.is_strong_instance_ref(), Some(true));
        assert_eq!(function.is_strong_instance_ref(), Some(false));
    };

    let f: NativeFunc<(), ()> = {
        let store = Store::default();
        let module = Module::new(&store, FUNCTION_WAT)?;
        let env = FunctionEnv::default();

        let instance = Instance::new(
            &module,
            &imports! {
                "env" => {
                    "host_fn" => Function::new_native_with_env(&store, env, host_fn)
                }
            },
        )?;

        {
            let function = instance.exports.get_function("call_host_fn")?;
            assert_eq!(function.is_strong_instance_ref(), Some(true));
        }

        let f: NativeFunc<(), ()> = instance.exports.get_native_function("call_host_fn")?;
        f.call()?;
        f
    };
    f.call()?;

    Ok(())
}

#[test]
fn strong_weak_behavior_works_native_function() -> Result<()> {
    #[derive(Clone, WasmerEnv, Default)]
    struct FunctionEnv {
        #[wasmer(export)]
        call_host_fn: LazyInit<NativeFunc<(), ()>>,
    }

    let host_fn = |env: &FunctionEnv| {
        let function = env.call_host_fn_ref().unwrap();
        assert_eq!(function.is_strong_instance_ref(), Some(false));
        let function_clone = function.clone();
        assert_eq!(function_clone.is_strong_instance_ref(), Some(true));
        assert_eq!(function.is_strong_instance_ref(), Some(false));
    };

    let f: NativeFunc<(), ()> = {
        let store = Store::default();
        let module = Module::new(&store, FUNCTION_WAT)?;
        let env = FunctionEnv::default();

        let instance = Instance::new(
            &module,
            &imports! {
                "env" => {
                    "host_fn" => Function::new_native_with_env(&store, env, host_fn)
                }
            },
        )?;

        {
            let function: NativeFunc<(), ()> =
                instance.exports.get_native_function("call_host_fn")?;
            assert_eq!(function.is_strong_instance_ref(), Some(true));
        }

        let f: NativeFunc<(), ()> = instance.exports.get_native_function("call_host_fn")?;
        f.call()?;
        f
    };
    f.call()?;

    Ok(())
}
