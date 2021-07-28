//! This file is mainly to assure specific issues are working well
use anyhow::Result;
use wasmer::*;

/// Corruption of WasmerEnv when using call indirect.
///
/// Note: this one is specific to Singlepass, but we want to test in all
/// available compilers.
///
/// https://github.com/wasmerio/wasmer/issues/2329
#[compiler_test(issues)]
fn issue_2329(mut config: crate::Config) -> Result<()> {
    let store = config.store();

    #[derive(Clone, Default, WasmerEnv)]
    pub struct Env {
        #[wasmer(export)]
        memory: LazyInit<Memory>,
    }

    impl Env {
        pub fn new() -> Self {
            Self {
                memory: LazyInit::new(),
            }
        }
    }

    pub fn read_memory(env: &Env, guest_ptr: u32) -> u32 {
        dbg!(env.memory_ref());
        dbg!(guest_ptr);
        0
    }

    let wat = r#"
    (module
        (type (;0;) (func (param i32) (result i32)))
        (type (;1;) (func))
        (import "env" "__read_memory" (func $__read_memory (type 0)))
        (func $read_memory (type 1)
          (drop
            (call $__read_memory
              (i32.const 2)))
          (drop
            (call $__read_memory
              (i32.const 1))))
        (table (;0;) 1 1 funcref)
        (memory (;0;) 16)
        (global (;0;) (mut i32) (i32.const 1048576))
        (global (;1;) i32 (i32.const 1048576))
        (global (;2;) i32 (i32.const 1048576))
        (export "memory" (memory 0))
        (export "read_memory" (func $read_memory))
        (export "__data_end" (global 1))
        (export "__heap_base" (global 2)))
      "#;
    let module = Module::new(&store, wat)?;
    let env = Env::new();
    let imports: ImportObject = imports! {
        "env" => {
            "__read_memory" => Function::new_native_with_env(
                &store,
                env.clone(),
                read_memory
            ),
        }
    };
    let instance = Instance::new(&module, &imports)?;
    instance.exports.get_function("read_memory")?.call(&[])?;
    Ok(())
}
