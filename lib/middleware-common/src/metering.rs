use wasmer_runtime_core::{
    codegen::{Event, EventSink, FunctionMiddleware, InternalEvent},
    module::ModuleInfo,
    vm::{Ctx, InternalField},
    wasmparser::{Operator, Type as WpType},
    Instance,
};

static INTERNAL_FIELD: InternalField = InternalField::allocate();

/// Metering is a compiler middleware that calculates the cost of WebAssembly instructions at compile
/// time and will count the cost of executed instructions at runtime. Within the Metering functionality,
/// this instruction cost is called `points`.
///
/// The Metering struct takes a `limit` parameter which is the maximum number of points which can be
/// used by an instance during a function call. If this limit is exceeded, the function call will
/// trap. Each instance has a `points_used` field which can be used to track points used during
/// a function call and should be set back to zero after a function call.
///
/// Each compiler backend with Metering enabled should produce the same cost used at runtime for
/// the same function calls so we can say that the metering is deterministic.
///
pub struct Metering {
    limit: u64,
    current_block: u64,
}

impl Metering {
    pub fn new(limit: u64) -> Metering {
        Metering {
            limit,
            current_block: 0,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ExecutionLimitExceededError;

impl FunctionMiddleware for Metering {
    type Error = String;
    fn feed_event<'a, 'b: 'a>(
        &mut self,
        op: Event<'a, 'b>,
        _module_info: &ModuleInfo,
        sink: &mut EventSink<'a, 'b>,
    ) -> Result<(), Self::Error> {
        match op {
            Event::Internal(InternalEvent::FunctionBegin(_)) => {
                self.current_block = 0;
            }
            Event::Wasm(&ref op) | Event::WasmOwned(ref op) => {
                self.current_block += 1;
                match *op {
                    Operator::Loop { .. }
                    | Operator::Block { .. }
                    | Operator::End
                    | Operator::If { .. }
                    | Operator::Else
                    | Operator::Unreachable
                    | Operator::Br { .. }
                    | Operator::BrTable { .. }
                    | Operator::BrIf { .. }
                    | Operator::Call { .. }
                    | Operator::CallIndirect { .. }
                    | Operator::Return => {
                        sink.push(Event::Internal(InternalEvent::GetInternal(
                            INTERNAL_FIELD.index() as _,
                        )));
                        sink.push(Event::WasmOwned(Operator::I64Const {
                            value: self.current_block as i64,
                        }));
                        sink.push(Event::WasmOwned(Operator::I64Add));
                        sink.push(Event::Internal(InternalEvent::SetInternal(
                            INTERNAL_FIELD.index() as _,
                        )));
                        self.current_block = 0;
                    }
                    _ => {}
                }
                match *op {
                    Operator::Br { .. }
                    | Operator::BrTable { .. }
                    | Operator::BrIf { .. }
                    | Operator::Call { .. }
                    | Operator::CallIndirect { .. } => {
                        sink.push(Event::Internal(InternalEvent::GetInternal(
                            INTERNAL_FIELD.index() as _,
                        )));
                        sink.push(Event::WasmOwned(Operator::I64Const {
                            value: self.limit as i64,
                        }));
                        sink.push(Event::WasmOwned(Operator::I64GeU));
                        sink.push(Event::WasmOwned(Operator::If {
                            ty: WpType::EmptyBlockType,
                        }));
                        sink.push(Event::Internal(InternalEvent::Breakpoint(Box::new(
                            move |ctx| unsafe {
                                (ctx.throw)(Box::new(ExecutionLimitExceededError));
                            },
                        ))));
                        sink.push(Event::WasmOwned(Operator::End));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        sink.push(op);
        Ok(())
    }
}

/// Returns the number of points used by an Instance.
pub fn get_points_used(instance: &Instance) -> u64 {
    instance.get_internal(&INTERNAL_FIELD)
}

/// Sets the number of points used by an Instance.
pub fn set_points_used(instance: &mut Instance, value: u64) {
    instance.set_internal(&INTERNAL_FIELD, value);
}

/// Returns the number of points used in a Ctx.
pub fn get_points_used_ctx(ctx: &Ctx) -> u64 {
    ctx.get_internal(&INTERNAL_FIELD)
}

/// Sets the number of points used in a Ctx.
pub fn set_points_used_ctx(ctx: &mut Ctx, value: u64) {
    ctx.set_internal(&INTERNAL_FIELD, value);
}

#[cfg(all(test, feature = "singlepass"))]
mod tests {
    use super::*;
    use wabt::wat2wasm;

    use wasmer_runtime_core::{backend::Compiler, compile_with, imports, Func};

    #[cfg(feature = "llvm")]
    fn get_compiler(limit: u64) -> impl Compiler {
        use wasmer_llvm_backend::code::LLVMModuleCodeGenerator;
        use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
        let c: StreamingCompiler<LLVMModuleCodeGenerator, _, _, _, _> =
            StreamingCompiler::new(move || {
                let mut chain = MiddlewareChain::new();
                chain.push(Metering::new(limit));
                chain
            });
        c
    }

    #[cfg(feature = "singlepass")]
    fn get_compiler(limit: u64) -> impl Compiler {
        use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
        use wasmer_singlepass_backend::ModuleCodeGenerator as SinglePassMCG;
        let c: StreamingCompiler<SinglePassMCG, _, _, _, _> = StreamingCompiler::new(move || {
            let mut chain = MiddlewareChain::new();
            chain.push(Metering::new(limit));
            chain
        });
        c
    }

    #[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
    fn get_compiler(_limit: u64) -> impl Compiler {
        panic!("compiler not specified, activate a compiler via features");
        use wasmer_clif_backend::CraneliftCompiler;
        CraneliftCompiler::new()
    }

    #[cfg(feature = "clif")]
    fn get_compiler(_limit: u64) -> impl Compiler {
        panic!("cranelift does not implement metering");
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

        // verify is uses the correct number of points
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

        // verify is uses the correct number of points
        assert_eq!(get_points_used(&instance), 109); // Used points will be slightly more than `limit` because of the way we do gas checking.
    }

}
