#[cfg(all(test, any(feature = "singlepass", feature = "llvm")))]
mod tests {
    use wabt::wat2wasm;

    use wasmer_middleware_common::metering::*;
    use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
    use wasmer_runtime_core::{backend::Compiler, compile_with, imports, Func};

    #[cfg(feature = "llvm")]
    fn get_compiler(limit: u64) -> impl Compiler {
        use wasmer_llvm_backend::ModuleCodeGenerator as LLVMMCG;
        let c: StreamingCompiler<LLVMMCG, _, _, _, _> = StreamingCompiler::new(move || {
            let mut chain = MiddlewareChain::new();
            chain.push(Metering::new(limit));
            chain
        });
        c
    }

    #[cfg(feature = "singlepass")]
    fn get_compiler(limit: u64) -> impl Compiler {
        use wasmer_singlepass_backend::ModuleCodeGenerator as SinglePassMCG;
        let c: StreamingCompiler<SinglePassMCG, _, _, _, _> = StreamingCompiler::new(move || {
            let mut chain = MiddlewareChain::new();
            chain.push(Metering::new(limit));
            chain
        });
        c
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    compile_error!("compiler not specified, activate a compiler via features");

    #[cfg(feature = "clif")]
    fn get_compiler(_limit: u64) -> impl Compiler {
        compile_error!("cranelift does not implement metering");
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
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
    static WAT: &'static str = r#"
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
    fn test_points_reduced_after_call() {
        let wasm_binary = wat2wasm(WAT).unwrap();

        let limit = 100u64;

        let module = compile_with(&wasm_binary, &get_compiler(limit)).unwrap();

        let import_object = imports! {};
        let mut instance = module.instantiate(&import_object).unwrap();

        set_points_used(&mut instance, 0u64);

        let add_to: Func<(i32, i32), i32> = instance.func("add_to").unwrap();
        let value = add_to.call(3, 4).unwrap();

        // verify it returns the correct value
        assert_eq!(value, 7);

        // verify it used the correct number of points
        assert_eq!(get_points_used(&instance), 74);
    }

    #[test]
    fn test_traps_after_costly_call() {
        use wasmer_runtime_core::error::RuntimeError;
        let wasm_binary = wat2wasm(WAT).unwrap();

        let limit = 100u64;

        let module = compile_with(&wasm_binary, &get_compiler(limit)).unwrap();

        let import_object = imports! {};
        let mut instance = module.instantiate(&import_object).unwrap();

        set_points_used(&mut instance, 0u64);

        let add_to: Func<(i32, i32), i32> = instance.func("add_to").unwrap();
        let result = add_to.call(10_000_000, 4);

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
}
