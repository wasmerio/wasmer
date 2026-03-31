//! This file is mainly to assure specific issues are working well

use anyhow::{Context, Result};
use itertools::Itertools;
use wasmer::FunctionEnv;
use wasmer::*;

/// Corruption of WasmerEnv when using call indirect.
///
/// Note: this one is specific to Singlepass, but we want to test in all
/// available compilers.
///
/// https://github.com/wasmerio/wasmer/issues/2329
#[compiler_test(issues)]
fn issue_2329(mut config: crate::Config) -> Result<()> {
    let mut store = config.store();

    #[derive(Clone, Default)]
    pub struct Env {
        memory: Option<Memory>,
    }

    impl Env {
        pub fn new() -> Self {
            Self { memory: None }
        }
    }

    pub fn read_memory(mut ctx: FunctionEnvMut<Env>, guest_ptr: u32) -> u32 {
        dbg!(ctx.data().memory.as_ref());
        dbg!(guest_ptr);
        0
    }

    let wat = r#"
    (module
        (type (;0;) (func (param i32) (result i32)))
        (type (;1;) (func))
        (type (;2;) (func (param i32 i32) (result i32)))
        (import "env" "__read_memory" (func $__read_memory (type 0)))
        (func $read_memory (type 1)
          (drop
            (call $_ZN5other8dispatch17h053cb34ef5d0d7b0E
              (i32.const 1)
              (i32.const 2)))
          (drop
            (call $__read_memory
              (i32.const 1))))
        (func $_ZN5other8dispatch17h053cb34ef5d0d7b0E (type 2) (param i32 i32) (result i32)
          (call_indirect (type 0)
            (local.get 1)
            (local.get 0)))
        (table (;0;) 2 2 funcref)
        (memory (;0;) 16)
        (global (;0;) (mut i32) (i32.const 1048576))
        (global (;1;) i32 (i32.const 1048576))
        (global (;2;) i32 (i32.const 1048576))
        (export "memory" (memory 0))
        (export "read_memory" (func $read_memory))
        (export "__data_end" (global 1))
        (export "__heap_base" (global 2))
        (elem (;0;) (i32.const 1) func $__read_memory))
    "#;
    let module = Module::new(&store, wat)?;
    let env = Env::new();
    let mut env = FunctionEnv::new(&mut store, env);
    let imports: Imports = imports! {
        "env" => {
            "__read_memory" => Function::new_typed_with_env(
                &mut store,
                &env,
                read_memory
            ),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;
    instance
        .exports
        .get_function("read_memory")?
        .call(&mut store, &[])?;
    Ok(())
}

#[compiler_test(issues)]
fn call_with_static_data_pointers(mut config: crate::Config) -> Result<()> {
    let mut store = config.store();

    #[derive(Clone)]
    pub struct Env {
        memory: Option<Memory>,
    }

    #[allow(clippy::too_many_arguments)]
    pub fn banana(
        mut ctx: FunctionEnvMut<Env>,
        a: u64,
        b: u64,
        c: u64,
        d: u64,
        e: u64,
        f: u64,
        g: u64,
        h: u64,
    ) -> u64 {
        println!("{:?}", (a, b, c, d, e, f, g, h));
        let mut buf = vec![0; d as usize];
        let memory = ctx.data().memory.as_ref().unwrap().clone();
        memory.view(&ctx).read(e, &mut buf).unwrap();
        let input_string = std::str::from_utf8(&buf).unwrap();
        assert_eq!(input_string, "bananapeach");
        0
    }

    pub fn mango(ctx: FunctionEnvMut<Env>, a: u64) {}

    pub fn chaenomeles(ctx: FunctionEnvMut<Env>, a: u64) -> u64 {
        0
    }

    pub fn peach(ctx: FunctionEnvMut<Env>, a: u64, b: u64) -> u64 {
        0
    }

    pub fn gas(ctx: FunctionEnvMut<Env>, a: u32) {}

    let wat = r#"
    (module
      (type (;0;) (func (param i64)))
      (type (;1;) (func (param i64) (result i64)))
      (type (;2;) (func (param i64 i64) (result i64)))
      (type (;3;) (func (param i64 i64 i64 i64 i64 i64 i64 i64) (result i64)))
      (type (;4;) (func))
      (import "env" "mango" (func (;0;) (type 0)))
      (import "env" "chaenomeles" (func (;1;) (type 1)))
      (import "env" "peach" (func (;2;) (type 2)))
      (import "env" "banana" (func (;3;) (type 3)))
      (import "env" "memory" (memory (;0;) 1024 2048))
      (func (;4;) (type 4)
        (local i32 i64)
        global.get 0
        i32.const 32
        i32.sub
        local.tee 0
        global.set 0
        local.get 0
        i32.const 8
        i32.add
        i64.const 0
        i64.store
        local.get 0
        i64.const 0
        i64.store
        i64.const 0
        call 0
        i64.const 0
        call 1
        local.set 1
        local.get 0
        i64.const 0
        i64.store offset=24
        local.get 0
        i64.const 0
        i64.store offset=16
        i64.const 0
        i64.const 0
        call 2
        local.get 1
        local.get 0
        i64.extend_i32_u
        i64.const 11
        i32.const 1048576
        i64.extend_i32_u
        i64.const 0
        i64.const 0
        local.get 0
        i32.const 16
        i32.add
        i64.extend_i32_u
        call 3
        return)
      (global (;0;) (mut i32) (i32.const 1048576))
      (global (;1;) i32 (i32.const 1048587))
      (global (;2;) i32 (i32.const 1048592))
      (global (;3;) (mut i32) (i32.const 0))
      (export "memory" (memory 0))
      (export "repro" (func 4))
      (export "__data_end" (global 1))
      (export "__heap_base" (global 2))
      (data (;0;) (i32.const 1048576) "bananapeach"))
    "#;

    let module = Module::new(&store, wat)?;
    let env = Env { memory: None };
    let mut env = FunctionEnv::new(&mut store, env);
    let memory = Memory::new(
        &mut store,
        MemoryType::new(Pages(1024), Some(Pages(2048)), false),
    )
    .unwrap();
    env.as_mut(&mut store).memory = Some(memory.clone());
    let mut exports = Exports::new();
    exports.insert("memory", memory);
    exports.insert(
        "banana",
        Function::new_typed_with_env(&mut store, &env, banana),
    );
    exports.insert(
        "peach",
        Function::new_typed_with_env(&mut store, &env, peach),
    );
    exports.insert(
        "chaenomeles",
        Function::new_typed_with_env(&mut store, &env, chaenomeles),
    );
    exports.insert(
        "mango",
        Function::new_typed_with_env(&mut store, &env, mango),
    );
    exports.insert("gas", Function::new_typed_with_env(&mut store, &env, gas));
    let mut imports = Imports::new();
    imports.register_namespace("env", exports);
    let instance = Instance::new(&mut store, &module, &imports)?;
    instance
        .exports
        .get_function("repro")?
        .call(&mut store, &[])?;
    Ok(())
}

/// Exhaustion of GPRs when calling a function with many floating point arguments
///
/// Note: this one is specific to Singlepass, but we want to test in all
/// available compilers.
#[compiler_test(issues)]
fn regression_gpr_exhaustion_for_calls(mut config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
        (module
          (type (;0;) (func (param f64) (result i32)))
          (type (;1;) (func (param f64 f64 f64 f64 f64 f64)))
          (func (;0;) (type 0) (param f64) (result i32)
            local.get 0
            local.get 0
            local.get 0
            local.get 0
            f64.const 0
            f64.const 0
            f64.const 0
            f64.const 0
            f64.const 0
            f64.const 0
            f64.const 0
            i32.const 0
            call_indirect (type 0)
            call_indirect (type 1)
            drop
            drop
            drop
            drop
            i32.const 0)
          (table (;0;) 1 1 funcref))
    "#;
    let mut env = FunctionEnv::new(&mut store, ());
    let module = Module::new(&store, wat)?;
    let imports: Imports = imports! {};
    let instance = Instance::new(&mut store, &module, &imports)?;
    Ok(())
}

#[compiler_test(issues)]
fn test_start(mut config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let mut env = FunctionEnv::new(&mut store, ());
    let imports: Imports = imports! {};
    let wat = r#"
    (module (func $main (unreachable)) (start $main))
    "#;
    let module = Module::new(&store, wat)?;
    let instance = Instance::new(&mut store, &module, &imports);
    assert!(instance.is_err());
    if let InstantiationError::Start(err) = instance.unwrap_err() {
        assert_eq!(err.message(), "unreachable");
    } else {
        panic!("_start should have failed with an unreachable error")
    }

    Ok(())
}

#[compiler_test(issues)]
fn test_popcnt(mut config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let mut env = FunctionEnv::new(&mut store, ());
    let imports: Imports = imports! {};

    let wat = r#"
    (module
        (func $popcnt_i32 (export "popcnt_i32") (param i32) (result i32)
            local.get 0
            i32.popcnt
        )
        (func $popcnt_i64 (export "popcnt_i64") (param i64) (result i64)
            local.get 0
            i64.popcnt
        )
    )"#;

    let module = Module::new(&store, wat)?;
    let instance = Instance::new(&mut store, &module, &imports)?;

    let popcnt_i32 = instance.exports.get_function("popcnt_i32").unwrap();
    let popcnt_i64 = instance.exports.get_function("popcnt_i64").unwrap();

    let get_next_number_i32 = |mut num: i32| {
        num ^= num << 13;
        num ^= num >> 17;
        num ^= num << 5;
        num
    };
    let get_next_number_i64 = |mut num: i64| {
        num ^= num << 34;
        num ^= num >> 40;
        num ^= num << 7;
        num
    };

    let mut num = 1;
    for _ in 1..10000 {
        let result = popcnt_i32.call(&mut store, &[Value::I32(num)]).unwrap();
        assert_eq!(
            &Value::I32(num.count_ones() as i32),
            result.first().unwrap()
        );
        num = get_next_number_i32(num);
    }

    let mut num = 1;
    for _ in 1..10000 {
        let result = popcnt_i64.call(&mut store, &[Value::I64(num)]).unwrap();
        assert_eq!(
            &Value::I64(num.count_ones() as i64),
            result.first().unwrap()
        );
        num = get_next_number_i64(num);
    }

    Ok(())
}

/// Create a large number of local (more than 0x1_0000 bytes, thats 32*16 i64 + 1)
/// to trigger an issue in the arm64 singlepass compiler
/// sequence
///   mov x17, #0x1010
///   sub xsp, xsp, x17
/// will tranform to
///   mov x17, #0x1010
///   sub xzr, xzr, x17
/// and the locals
/// on stack can get corrupted by subsequent calls if they also have locals on stack
#[compiler_test(issues)]
fn large_number_local(mut config: crate::Config) -> Result<()> {
    let mut store = config.store();
    let wat = r#"
      (module
        (func (;0;)
          (local i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
          i64.const 0
          i64.const 5555
          i64.add
          local.set 8
          i64.const 0
          i64.const 5555
          i64.add
          local.set 9
          i64.const 0
          i64.const 5555
          i64.add
          local.set 10
        )
        (func $large_local (export "large_local") (result i64)
          (local
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64
          )
          (local.set 15 (i64.const 1))
          (call 0)
          local.get 6
          local.get 7
          i64.add
          local.get 8
          i64.add
          local.get 9
          i64.add
          local.get 10
          i64.add
          local.get 11
          i64.add
          local.get 12
          i64.add
          local.get 13
          i64.add
          local.get 14
          i64.add
          local.get 15
          i64.add
          local.get 16
          i64.add
          local.get 512
          i64.add
        )
      )
    "#;
    let mut env = FunctionEnv::new(&mut store, ());
    let module = Module::new(&store, wat)?;
    let imports: Imports = imports! {};
    let instance = Instance::new(&mut store, &module, &imports)?;
    let result = instance
        .exports
        .get_function("large_local")?
        .call(&mut store, &[])
        .unwrap();
    assert_eq!(&Value::I64(1_i64), result.first().unwrap());
    Ok(())
}

// TODO: the tests fails on RISC-V as the `j` instruction can reach only +- 1MiB offset.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[compiler_test(issues)]
/// Singlepass panics on aarch64 for long relocations.
///
/// Note: this one is specific to Singlepass, but we want to test in all
/// available compilers.
///
/// https://github.com/wasmerio/wasmer/issues/4519
fn issue_4519(mut config: crate::Config) -> Result<()> {
    let wasm = include_bytes!("./data/4519_singlepass_panic.wasm");

    let mut store = config.store();
    let module = Module::new(&store, wasm)?;

    Ok(())
}

// TODO: the tests fails on RISC-V as the `j` instruction can reach only +- 1MiB offset.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[compiler_test(issues)]
/// Singlepass panics on aarch64 for long relocations.
/// This test specifically targets the emission of sdiv64, srem64, urem64 binops.
///
/// Note: this one is specific to Singlepass, but we want to test in all
/// available compilers.
///
/// https://github.com/wasmerio/wasmer/issues/4519
fn issue_4519_sdiv64_srem64_urem64(mut config: crate::Config) -> Result<()> {
    const REPEATS_TO_REPRODUCE: usize = 30_000;

    let ops = ["i64.div_s", "i64.rem_s", "i64.rem_u"];

    for op in ops {
        let sdiv64 = format!(
            r#"
            i64.const 3155225962131072202
            i64.const -6717269760755396770
            {op}
            drop
        "#
        );

        let wat = format!(
            r#"
        (module
            (func (;0;)
                {}
            )
        )
        "#,
            sdiv64.repeat(REPEATS_TO_REPRODUCE)
        );

        let mut store = config.store();
        let module = Module::new(&store, wat)?;
    }

    Ok(())
}

#[compiler_test(issues)]
/// Singlepass panics when encountering ref types.
///
/// Note: this one is specific to Singlepass, but we want to test in all
/// available compilers.
///
/// Note: for now, we don't want to implement reference types, we just don't want singlepass to
/// panic.
///
/// https://github.com/wasmerio/wasmer/issues/5309
fn issue_5309_reftype_panic(mut config: crate::Config) -> Result<()> {
    let wat = r#"
      (module
        (type $x1 (func (param funcref)))
        (import "env" "abort" (func $f (type $x1)))
      )
    "#
    .to_string();

    let mut store = config.store();
    let _ = Module::new(&store, wat);

    Ok(())
}

#[compiler_test(issues)]
fn issue_memory_atomic_notify_stack_offset(mut config: crate::Config) -> Result<()> {
    let store = config.store();
    let wat = r#"
    (module
      (table 1 externref)
      (memory 7)
      (func
        loop
          table.size
          table.size
          memory.atomic.notify
          unreachable
        end))
    "#;

    let _module = Module::new(&store, wat)?;
    Ok(())
}

fn gen_wat_sum_function(arguments: usize) -> String {
    assert!(arguments > 0);
    let arg_types = std::iter::repeat_n("i64", arguments).collect_vec();
    let params = (0..arguments)
        .map(|idx| format!("(param $p{} i64)", idx + 1))
        .collect_vec();
    let fn_body = (2..=arguments)
        .map(|idx| format!("local.get $p{idx}\ni64.add"))
        .collect_vec();

    format!(
        r#"
    (module
    (type $sum_t (func (param {}) (result i64)))
    (func $sum_f (type $sum_t)
    {}
    (result i64)
    local.get $p1
    {}
    )
    (export "sum" (func $sum_f)))
    "#,
        arg_types.join(" "),
        params.join(" "),
        fn_body.join("\n")
    )
}

#[compiler_test(issues)]
fn huge_number_of_arguments_fn(
    mut config: crate::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    for params in [1, 10, 100, 500, 1000] {
        println!("Testing sum fn with {params} parameters");
        let mut store = config.store();
        let wat_body = gen_wat_sum_function(params as usize);
        let wat = wat2wasm(wat_body.as_bytes())
            .with_context(|| format!("Cannot build sum function with {params} parameters"))?;

        let mut env = FunctionEnv::new(&mut store, ());
        let module = Module::new(&store, wat).unwrap();
        let imports: Imports = imports! {};
        let instance = Instance::new(&mut store, &module, &imports)?;
        let args = (1..=params).map(Value::I64).collect_vec();
        let result = instance
            .exports
            .get_function("sum")?
            .call(&mut store, &args)
            .unwrap();
        assert_eq!(&Value::I64((1..=params).sum()), result.first().unwrap());
    }

    Ok(())
}

#[cfg(feature = "llvm")]
#[compiler_test(issues)]
fn compiler_debug_dir_test(mut config: crate::Config) {
    use tempfile::TempDir;
    use wasmer_compiler::EngineBuilder;
    use wasmer_compiler_llvm::LLVMCallbacks;

    let mut compiler_config = wasmer_compiler_llvm::LLVM::default();
    let temp = TempDir::new().expect("temp folder creation failed");
    compiler_config.callbacks(Some(LLVMCallbacks::new(temp.path().to_path_buf()).unwrap()));
    let mut store = Store::new(EngineBuilder::new(compiler_config));

    let mut wat = include_str!("../wast/wasmer/fac.wast").to_string();
    wat.truncate(
        wat.find("(assert_return")
            .expect("assert expected in the test"),
    );

    assert!(Module::new(&store, wat).is_ok());
}

#[compiler_test(issues)]
fn issue_5795_memory_reset_size(mut config: crate::Config) {
    let wasm_bytes = wat2wasm(
        r#"
(module
   (memory (export "memory") 1 65536)
   (func (export "mem_size") (result i32)
       memory.size)
   (func (export "grow") (param i32) (result i32)
       local.get 0
       memory.grow))
"#
        .as_bytes(),
    )
    .expect("wat2wasm must succeed");

    let mut store = config.store();
    let module = Module::new(&store, wasm_bytes).unwrap();
    let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();

    let _ = memory.grow(&mut store, 999).unwrap();
    memory.reset(&mut store);

    assert_eq!(memory.size(&store).bytes().0, 0);
    assert_eq!(memory.view(&store).size().0, 0);

    assert_eq!(memory.grow(&mut store, 1).unwrap().0, 0);

    assert_eq!(memory.view(&store).size().0, 1);
}

#[cfg(not(target_os = "windows"))]
#[compiler_test(issues)]
fn issue_6004_exception(mut config: crate::Config) {
    let wasm_bytes = wat2wasm(
        r#"
(module
  (tag $e)

  (func (export "throw-expect-42") (result i32)
    (block $h1 (result exnref)
      (try_table (result exnref) (catch_all_ref $h1)
        (block $h2 (result exnref)
          (try_table (result exnref) (catch_ref $e $h2)
            (throw $e)
          )
        )
        (i32.const 42)
        (return)
      )
    )
    (drop)
    (i32.const 1)
  )
)
"#
        .as_bytes(),
    )
    .expect("wat2wasm must succeed");

    let mut store = config.store();
    let module = match Module::new(&store, wasm_bytes) {
        Err(CompileError::Validate(message))
            if message.contains("exceptions proposal not enabled") =>
        {
            // Skip the test in that case.
            return;
        }
        Ok(module) => module,
        _ => unreachable!(),
    };
    let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
    let result = instance
        .exports
        .get_function("throw-expect-42")
        .unwrap()
        .call(&mut store, &[])
        .unwrap();
    assert_eq!(&Value::I32(42), result.first().unwrap());
}

#[cfg(not(target_os = "windows"))]
#[compiler_test(issues)]
fn issue_5719_shared_catch_clause_block(mut config: crate::Config) {
    let wasm_bytes = wat2wasm(
        r#"
(module
    (tag $err (param))
    (tag $err2 (param))
    (export "f" (func $f))
    (func $f (result i32)
        block
            block
                try_table (catch $err 0) (catch $err2 0)
                    throw $err2
                end
                unreachable
            end
            i32.const 42
            return
        end
        i32.const 13
        return
    )
)
"#
        .as_bytes(),
    )
    .expect("wat2wasm must succeed");

    let mut store = config.store();
    let module = match Module::new(&store, wasm_bytes) {
        Err(CompileError::Validate(message))
            if message.contains("exceptions proposal not enabled") =>
        {
            // Skip the test in that case.
            return;
        }
        Ok(module) => module,
        _ => unreachable!(),
    };
    let instance = Instance::new(&mut store, &module, &imports! {}).unwrap();
    let result = instance
        .exports
        .get_function("f")
        .unwrap()
        .call(&mut store, &[])
        .unwrap();
    assert_eq!(&Value::I32(42), result.first().unwrap());
}

#[compiler_test(issues)]
fn issue_4169_funcref_externref_import(mut config: crate::Config) -> Result<()> {
    let wasm_bytes = wat2wasm(
        r#"
        (module
            (type $t0 (func (param funcref externref)))
            (import "" "" (func $hello (type $t0)))
        )
        "#
        .as_bytes(),
    )
    .unwrap();

    let mut store = config.store();
    let module = Module::new(&store, wasm_bytes).unwrap();
    let imports: Imports = imports! {
        "" => {
            "" => Function::new_typed(
                &mut store,
                |_fr: Option<Function>, _er: Option<ExternRef>| {},
            ),
        }
    };

    let _instance = Instance::new(&mut store, &module, &imports)?;

    Ok(())
}

#[cfg(feature = "llvm")]
#[compiler_test(issues)]
fn issue_return_call_import(mut config: crate::Config) -> Result<()> {
    if config.compiler != crate::Compiler::LLVM {
        return Ok(());
    }

    let mut features = wasmer::sys::Features::new();
    features.tail_call(true);
    config.set_features(features);

    let mut store = config.store();
    let wasm_bytes = wat2wasm(
        br#"
        (module
            (type $t0 (func (param i32) (result i32)))
            (import "env" "add_one" (func $add_one (type $t0)))
            (memory 1 1)
            (func (export "run") (param i32) (result i32)
                local.get 0
                return_call $add_one
            )
        )
        "#,
    )?;

    let module = Module::new(&store, wasm_bytes)?;
    let imports: Imports = imports! {
        "env" => {
            "add_one" => Function::new_typed(&mut store, |value: i32| value + 1),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let result = instance
        .exports
        .get_function("run")?
        .call(&mut store, &[Value::I32(41)])?;

    assert_eq!(&*result, &[Value::I32(42)]);

    Ok(())
}

#[cfg(feature = "llvm")]
#[compiler_test(issues)]
fn issue_return_call_indirect_mixed_local_import(mut config: crate::Config) -> Result<()> {
    if config.compiler != crate::Compiler::LLVM {
        return Ok(());
    }

    let mut features = wasmer::sys::Features::new();
    features.tail_call(true);
    config.set_features(features);

    let mut store = config.store();
    let wasm_bytes = wat2wasm(
        br#"
        (module
            (type $t0 (func (param i32) (result i32)))
            (import "env" "add_one" (func $add_one (type $t0)))
            (memory 1 1)
            (table 2 funcref)
            (elem (i32.const 0) func $add_one $add_ten)
            (func $add_ten (type $t0) (param i32) (result i32)
                local.get 0
                i32.const 10
                i32.add
            )
            (func $dispatch (param i32 i32) (result i32)
                local.get 0
                local.get 1
                return_call_indirect (type $t0)
            )
            (func (export "run") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                call $dispatch
            )
        )
        "#,
    )?;

    let module = Module::new(&store, wasm_bytes)?;
    let imports: Imports = imports! {
        "env" => {
            "add_one" => Function::new_typed(&mut store, |value: i32| value + 1),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let run = instance.exports.get_function("run")?;

    let imported = run.call(&mut store, &[Value::I32(41), Value::I32(0)])?;
    assert_eq!(&*imported, &[Value::I32(42)]);

    let local = run.call(&mut store, &[Value::I32(32), Value::I32(1)])?;
    assert_eq!(&*local, &[Value::I32(42)]);

    Ok(())
}

#[cfg(feature = "llvm")]
#[compiler_test(issues)]
fn issue_return_call_sret_multivalue(mut config: crate::Config) -> Result<()> {
    if config.compiler != crate::Compiler::LLVM {
        return Ok(());
    }

    let mut features = wasmer::sys::Features::new();
    features.tail_call(true);
    config.set_features(features);

    let mut store = config.store();
    let wasm_bytes = wat2wasm(
        br#"
        (module
            (type $t0 (func (result i64 i64 i64)))
            (func $values (type $t0) (result i64 i64 i64)
                i64.const 11
                i64.const 22
                i64.const 33
            )
            (func (export "run") (type $t0) (result i64 i64 i64)
                return_call $values
            )
        )
        "#,
    )?;

    let module = Module::new(&store, wasm_bytes)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let run: TypedFunction<(), (i64, i64, i64)> =
        instance.exports.get_typed_function(&store, "run")?;

    assert_eq!(run.call(&mut store)?, (11, 22, 33));

    Ok(())
}

#[cfg(feature = "llvm")]
#[compiler_test(issues)]
fn issue_return_call_indirect_import(mut config: crate::Config) -> Result<()> {
    // Reproducer for LLVM `musttail` selection based only on wasm signatures:
    // the caller has static memory and therefore an `m0` parameter, but the
    // imported target does not.
    if config.compiler != crate::Compiler::LLVM {
        return Ok(());
    }

    let mut features = wasmer::sys::Features::new();
    features.tail_call(true);
    config.set_features(features);

    let mut store = config.store();
    let wasm_bytes = wat2wasm(
        br#"
        (module
            (type $t0 (func (param i32) (result i32)))
            (import "env" "add_one" (func $add_one (type $t0)))
            (memory 1 1)
            (table 1 funcref)
            (elem (i32.const 0) func $add_one)
            (func $dispatch (param i32) (result i32)
                local.get 0
                i32.const 0
                return_call_indirect (type $t0)
            )
            (func (export "run") (param i32) (result i32)
                local.get 0
                call $dispatch
            )
        )
        "#,
    )?;

    let module = Module::new(&store, wasm_bytes)?;
    let imports: Imports = imports! {
        "env" => {
            "add_one" => Function::new_typed(&mut store, |value: i32| value + 1),
        }
    };
    let instance = Instance::new(&mut store, &module, &imports)?;
    let result = instance
        .exports
        .get_function("run")?
        .call(&mut store, &[Value::I32(41)])?;

    assert_eq!(&*result, &[Value::I32(42)]);

    Ok(())
}

#[compiler_test(issues)]
fn issue_6334_foldable_comparison_expressions(mut config: crate::Config) -> Result<()> {
    if config.compiler != crate::Compiler::Singlepass {
        return Ok(());
    }

    let mut store = config.store();
    let wasm_bytes = wat2wasm(
        br#"
        (module
            (func (export "i32_ge_u_false") (result i32)
                i32.const 1
                i32.const 2
                i32.ge_u
            )
            (func (export "i32_ge_u_true") (result i32)
                i32.const 2
                i32.const 1
                i32.ge_u
            )
            (func (export "i32_gt_u_false") (result i32)
                i32.const 1
                i32.const 2
                i32.gt_u
            )
            (func (export "i32_gt_u_true") (result i32)
                i32.const 2
                i32.const 1
                i32.gt_u
            )
            (func (export "i32_eq_false") (result i32)
                i32.const 1
                i32.const 2
                i32.eq
            )
            (func (export "i32_eq_true") (result i32)
                i32.const 2
                i32.const 2
                i32.eq
            )
            (func (export "i32_ne_false") (result i32)
                i32.const 2
                i32.const 2
                i32.ne
            )
            (func (export "i32_ne_true") (result i32)
                i32.const 1
                i32.const 2
                i32.ne
            )
            (func (export "i64_ge_u_false") (result i32)
                i64.const 1
                i64.const 2
                i64.ge_u
            )
            (func (export "i64_ge_u_true") (result i32)
                i64.const 2
                i64.const 1
                i64.ge_u
            )
            (func (export "i64_gt_u_false") (result i32)
                i64.const 1
                i64.const 2
                i64.gt_u
            )
            (func (export "i64_gt_u_true") (result i32)
                i64.const 2
                i64.const 1
                i64.gt_u
            )
            (func (export "i64_eq_false") (result i32)
                i64.const 1
                i64.const 2
                i64.eq
            )
            (func (export "i64_eq_true") (result i32)
                i64.const 2
                i64.const 2
                i64.eq
            )
            (func (export "i64_ne_false") (result i32)
                i64.const 2
                i64.const 2
                i64.ne
            )
            (func (export "i64_ne_true") (result i32)
                i64.const 1
                i64.const 2
                i64.ne
            )
        )
        "#,
    )?;

    let module = Module::new(&store, wasm_bytes)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;

    let i32_ge_u_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i32_ge_u_false")?;
    let i32_ge_u_true: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i32_ge_u_true")?;
    let i32_gt_u_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i32_gt_u_false")?;
    let i32_gt_u_true: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i32_gt_u_true")?;
    let i32_eq_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i32_eq_false")?;
    let i32_eq_true: TypedFunction<(), i32> =
        instance.exports.get_typed_function(&store, "i32_eq_true")?;
    let i32_ne_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i32_ne_false")?;
    let i32_ne_true: TypedFunction<(), i32> =
        instance.exports.get_typed_function(&store, "i32_ne_true")?;
    let i64_ge_u_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i64_ge_u_false")?;
    let i64_ge_u_true: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i64_ge_u_true")?;
    let i64_gt_u_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i64_gt_u_false")?;
    let i64_gt_u_true: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i64_gt_u_true")?;
    let i64_eq_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i64_eq_false")?;
    let i64_eq_true: TypedFunction<(), i32> =
        instance.exports.get_typed_function(&store, "i64_eq_true")?;
    let i64_ne_false: TypedFunction<(), i32> = instance
        .exports
        .get_typed_function(&store, "i64_ne_false")?;
    let i64_ne_true: TypedFunction<(), i32> =
        instance.exports.get_typed_function(&store, "i64_ne_true")?;

    assert_eq!(i32_ge_u_false.call(&mut store)?, 0);
    assert_eq!(i32_ge_u_true.call(&mut store)?, 1);
    assert_eq!(i32_gt_u_false.call(&mut store)?, 0);
    assert_eq!(i32_gt_u_true.call(&mut store)?, 1);
    assert_eq!(i32_eq_false.call(&mut store)?, 0);
    assert_eq!(i32_eq_true.call(&mut store)?, 1);
    assert_eq!(i32_ne_false.call(&mut store)?, 0);
    assert_eq!(i32_ne_true.call(&mut store)?, 1);
    assert_eq!(i64_ge_u_false.call(&mut store)?, 0);
    assert_eq!(i64_ge_u_true.call(&mut store)?, 1);
    assert_eq!(i64_gt_u_false.call(&mut store)?, 0);
    assert_eq!(i64_gt_u_true.call(&mut store)?, 1);
    assert_eq!(i64_eq_false.call(&mut store)?, 0);
    assert_eq!(i64_eq_true.call(&mut store)?, 1);
    assert_eq!(i64_ne_false.call(&mut store)?, 0);
    assert_eq!(i64_ne_true.call(&mut store)?, 1);

    Ok(())
}

#[cfg(feature = "singlepass")]
#[compiler_test(issues)]
fn singlepass_memory_trap(mut config: crate::Config) -> Result<()> {
    if config.compiler != crate::Compiler::Singlepass {
        return Ok(());
    }

    let mut compiler_config = wasmer_compiler_singlepass::Singlepass::default();
    compiler_config.strict_memory_boundary_checks(true);
    let mut store = Store::new(wasmer_compiler::EngineBuilder::new(compiler_config));

    let wasm_bytes = wat2wasm(
        r#"
(module
  (memory (export "memory") 1)
  (data (i32.const 65528) "\01\02\03\04\05\06\07\08")
  (func (export "run")
    i32.const 65529
    i32.const 0
    i32.const 1
    select
    i64.const 0
    i64.store)
)
"#
        .as_bytes(),
    )
    .expect("wat2wasm must succeed");

    let module = Module::new(&store, wasm_bytes)?;
    let instance = Instance::new(&mut store, &module, &imports! {})?;
    let memory = instance.exports.get_memory("memory")?;
    let run = instance.exports.get_function("run")?;

    let mut before = [0_u8; 8];
    memory.view(&store).read(65528, &mut before)?;
    assert_eq!(before, [1, 2, 3, 4, 5, 6, 7, 8]);

    let trap = run.call(&mut store, &[]);
    assert!(trap.is_err(), "expected out-of-bounds store to trap");

    let mut after = [0_u8; 8];
    memory.view(&store).read(65528, &mut after)?;
    assert_eq!(after, before);

    Ok(())
}
