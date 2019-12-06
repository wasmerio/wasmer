#[cfg(all(test, any(feature = "singlepass", feature = "llvm")))]
mod tests {
    use wabt::wat2wasm;

    use wasmer_middleware_common::metering::*;
    use wasmer_middleware_common::stack_limit::*;
    use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
    use wasmer_runtime_core::fault::{pop_code_version, push_code_version};
    use wasmer_runtime_core::state::CodeVersion;
    use wasmer_runtime_core::{
        backend::{Backend, Compiler},
        compile_with, imports, Func,
    };

    #[cfg(feature = "llvm")]
    fn metering_get_compiler(limit: u64) -> (impl Compiler, Backend) {
        use wasmer_llvm_backend::ModuleCodeGenerator as LLVMMCG;
        let c: StreamingCompiler<LLVMMCG, _, _, _, _> = StreamingCompiler::new(move || {
            let mut chain = MiddlewareChain::new();
            chain.push(Metering::new(limit));
            chain
        });
        (c, Backend::LLVM)
    }

    #[cfg(feature = "singlepass")]
    fn metering_get_compiler(limit: u64) -> (impl Compiler, Backend) {
        use wasmer_singlepass_backend::ModuleCodeGenerator as SinglePassMCG;
        let c: StreamingCompiler<SinglePassMCG, _, _, _, _> = StreamingCompiler::new(move || {
            let mut chain = MiddlewareChain::new();
            chain.push(Metering::new(limit));
            chain
        });
        (c, Backend::Singlepass)
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    compile_error!("compiler not specified, activate a compiler via features");

    #[cfg(feature = "clif")]
    fn metering_get_compiler(_limit: u64) -> (impl Compiler, Backend) {
        compile_error!("cranelift does not implement metering");
        use wasmer_clif_backend::CraneliftCompiler;
        (CraneliftCompiler::new(), Backend::Cranelift)
    }

    // Assemblyscript
    // export function add_to(x: i32, y: i32): i32 {
    //    for(var i = 0; i < x; i++){
    //      if(i % 1 == 0){
    //        y += i;
    //      } else {
    //        y *= i
    //      }
    //    }
    //    return y;
    // }
    static WAT_METERING: &'static str = r#"
        (module
          (type $t0 (func (param i32 i32) (result i32)))
          (type $t1 (func))
          (func $add_to (export "add_to") (type $t0) (param $p0 i32) (param $p1 i32) (result i32)
            (local $l0 i32)
            block $B0
              i32.const 0
              set_local $l0
              loop $L1
                get_local $l0
                get_local $p0
                i32.lt_s
                i32.eqz
                br_if $B0
                get_local $l0
                i32.const 1
                i32.rem_s
                i32.const 0
                i32.eq
                if $I2
                  get_local $p1
                  get_local $l0
                  i32.add
                  set_local $p1
                else
                  get_local $p1
                  get_local $l0
                  i32.mul
                  set_local $p1
                end
                get_local $l0
                i32.const 1
                i32.add
                set_local $l0
                br $L1
                unreachable
              end
              unreachable
            end
            get_local $p1)
          (func $f1 (type $t1))
          (table $table (export "table") 1 anyfunc)
          (memory $memory (export "memory") 0)
          (global $g0 i32 (i32.const 8))
          (elem (i32.const 0) $f1))
        "#;

    #[test]
    fn test_metering_points_reduced_after_call() {
        let wasm_binary = wat2wasm(WAT_METERING).unwrap();

        let limit = 100u64;

        let (compiler, backend_id) = metering_get_compiler(limit);
        let module = compile_with(&wasm_binary, &compiler).unwrap();

        let import_object = imports! {};
        let mut instance = module.instantiate(&import_object).unwrap();

        set_points_used(&mut instance, 0u64);

        let add_to: Func<(i32, i32), i32> = instance.func("add_to").unwrap();

        let cv_pushed = if let Some(msm) = instance.module.runnable_module.get_module_state_map() {
            push_code_version(CodeVersion {
                baseline: true,
                msm: msm,
                base: instance.module.runnable_module.get_code().unwrap().as_ptr() as usize,
                backend: backend_id,
            });
            true
        } else {
            false
        };

        let value = add_to.call(3, 4).unwrap();
        if cv_pushed {
            pop_code_version().unwrap();
        }

        // verify it returns the correct value
        assert_eq!(value, 7);

        // verify it used the correct number of points
        assert_eq!(get_points_used(&instance), 74);
    }

    #[test]
    fn test_metering_traps_after_costly_call() {
        use wasmer_runtime_core::error::RuntimeError;
        let wasm_binary = wat2wasm(WAT_METERING).unwrap();

        let limit = 100u64;

        let (compiler, backend_id) = metering_get_compiler(limit);
        let module = compile_with(&wasm_binary, &compiler).unwrap();

        let import_object = imports! {};
        let mut instance = module.instantiate(&import_object).unwrap();

        set_points_used(&mut instance, 0u64);

        let add_to: Func<(i32, i32), i32> = instance.func("add_to").unwrap();

        let cv_pushed = if let Some(msm) = instance.module.runnable_module.get_module_state_map() {
            push_code_version(CodeVersion {
                baseline: true,
                msm: msm,
                base: instance.module.runnable_module.get_code().unwrap().as_ptr() as usize,
                backend: backend_id,
            });
            true
        } else {
            false
        };
        let result = add_to.call(10_000_000, 4);
        if cv_pushed {
            pop_code_version().unwrap();
        }

        let err = result.unwrap_err();
        match err {
            RuntimeError::Error { data } => {
                assert!(data.downcast_ref::<ExecutionLimitExceededError>().is_some());
            }
            _ => unreachable!(),
        }

        // verify it used the correct number of points
        assert_eq!(get_points_used(&instance), 109); // Used points will be slightly more than `limit` because of the way we do gas checking.
    }

    #[cfg(feature = "singlepass")]
    fn stack_limit_get_compiler(config: StackLimitConfig) -> (impl Compiler, Backend) {
        use wasmer_singlepass_backend::ModuleCodeGenerator as SinglePassMCG;
        let c: StreamingCompiler<SinglePassMCG, _, _, _, _> = StreamingCompiler::new(move || {
            let mut chain = MiddlewareChain::new();
            chain.push(StackLimit::new(config.clone()));
            chain
        });
        (c, Backend::Singlepass)
    }

    #[cfg(feature = "llvm")]
    fn stack_limit_get_compiler(config: StackLimitConfig) -> (impl Compiler, Backend) {
        use wasmer_llvm_backend::ModuleCodeGenerator as LLVMMCG;
        let c: StreamingCompiler<LLVMMCG, _, _, _, _> = StreamingCompiler::new(move || {
            let mut chain = MiddlewareChain::new();
            chain.push(StackLimit::new(config.clone()));
            chain
        });
        (c, Backend::LLVM)
    }

    static WAT_STACK_LIMIT_STATIC_SLOTS: &'static str = r#"
    (module
      (func $main (export "main") (param $p0 i32) (param $p1 i32) (result i32)
        (local $l0 i32)
        (call $f1)
      )
      (func $f1 (result i32)
        (local $l0 i32)
        (local $l1 i32)
        (i32.const 0)
      )
    )
    "#;

    static WAT_STACK_LIMIT_CALL_DEPTH: &'static str = r#"
    (module
      (func $main (export "main") (param $p0 i32) (param $p1 i32) (result i32)
        (call $f1)
        (i32.const 0)
      )
      (func $f1)
    )
    "#;

    fn _test_stack_limit_call_once(wat: &str, config: StackLimitConfig) -> bool {
        let wasm_binary = wat2wasm(wat).unwrap();

        let (compiler, backend_id) = stack_limit_get_compiler(config);
        let module = compile_with(&wasm_binary, &compiler).unwrap();

        let import_object = imports! {};
        let mut instance = module.instantiate(&import_object).unwrap();

        let main_fn: Func<(i32, i32), i32> = instance.func("main").unwrap();

        let cv_pushed = if let Some(msm) = instance.module.runnable_module.get_module_state_map() {
            push_code_version(CodeVersion {
                baseline: true,
                msm: msm,
                base: instance.module.runnable_module.get_code().unwrap().as_ptr() as usize,
                backend: backend_id,
            });
            true
        } else {
            false
        };

        let result = main_fn.call(1, 1).is_ok();

        if cv_pushed {
            pop_code_version().unwrap();
        }

        result
    }
    #[test]
    fn test_stack_limit_static_slots() {
        assert_eq!(
            _test_stack_limit_call_once(
                WAT_STACK_LIMIT_STATIC_SLOTS,
                StackLimitConfig {
                    max_call_depth: None,
                    max_value_stack_depth: None,
                    max_static_slot_count: Some(4),
                }
            ),
            false
        );
        assert_eq!(
            _test_stack_limit_call_once(
                WAT_STACK_LIMIT_STATIC_SLOTS,
                StackLimitConfig {
                    max_call_depth: None,
                    max_value_stack_depth: None,
                    max_static_slot_count: Some(5),
                }
            ),
            true
        );
    }

    #[test]
    fn test_stack_limit_call_depth() {
        assert_eq!(
            _test_stack_limit_call_once(
                WAT_STACK_LIMIT_CALL_DEPTH,
                StackLimitConfig {
                    max_call_depth: Some(1),
                    max_value_stack_depth: None,
                    max_static_slot_count: None,
                }
            ),
            false
        );
        assert_eq!(
            _test_stack_limit_call_once(
                WAT_STACK_LIMIT_CALL_DEPTH,
                StackLimitConfig {
                    max_call_depth: Some(2),
                    max_value_stack_depth: None,
                    max_static_slot_count: None,
                }
            ),
            true
        );
    }
}
