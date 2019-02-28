use inkwell::{
    execution_engine::JitFunction,
    targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine},
    OptimizationLevel,
};
use wasmer_runtime_core::{
    backend::{Compiler, Token},
    cache::{Artifact, Error as CacheError},
    error::CompileError,
    module::ModuleInner,
};

mod backend;
mod code;
mod intrinsics;
mod read_info;
mod state;

pub struct LLVMCompiler {
    _private: (),
}

impl LLVMCompiler {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Compiler for LLVMCompiler {
    fn compile(&self, wasm: &[u8], _: Token) -> Result<ModuleInner, CompileError> {
        let (info, code_reader) = read_info::read_module(wasm).unwrap();
        let (module, intrinsics) = code::parse_function_bodies(&info, code_reader).unwrap();

        let backend = backend::LLVMBackend::new(module, intrinsics);

        // Create placeholder values here.
        let (protected_caller, cache_gen) = {
            use wasmer_runtime_core::backend::{
                sys::Memory, CacheGen, ProtectedCaller, UserTrapper,
            };
            use wasmer_runtime_core::cache::Error as CacheError;
            use wasmer_runtime_core::error::RuntimeResult;
            use wasmer_runtime_core::module::ModuleInfo;
            use wasmer_runtime_core::types::{FuncIndex, Value};
            use wasmer_runtime_core::vm;
            struct Placeholder;
            impl ProtectedCaller for Placeholder {
                fn call(
                    &self,
                    _module: &ModuleInner,
                    _func_index: FuncIndex,
                    _params: &[Value],
                    _import_backing: &vm::ImportBacking,
                    _vmctx: *mut vm::Ctx,
                    _: Token,
                ) -> RuntimeResult<Vec<Value>> {
                    Ok(vec![])
                }
                fn get_early_trapper(&self) -> Box<dyn UserTrapper> {
                    unimplemented!()
                }
            }
            impl CacheGen for Placeholder {
                fn generate_cache(
                    &self,
                    module: &ModuleInner,
                ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError> {
                    unimplemented!()
                }
            }

            (Box::new(Placeholder), Box::new(Placeholder))
        };

        Ok(ModuleInner {
            func_resolver: Box::new(backend),
            protected_caller,
            cache_gen,

            info,
        })
    }

    unsafe fn from_cache(&self, _artifact: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        unimplemented!("the llvm backend doesn't support caching yet")
    }
}

#[test]
fn test_read_module() {
    use std::mem::transmute;
    use wabt::wat2wasm;
    use wasmer_runtime_core::{structures::TypedIndex, types::LocalFuncIndex, vm, vmcalls};
    // let wasm = include_bytes!("../../spectests/examples/simple/simple.wasm") as &[u8];
    let wat = r#"
        (module
        (type $t0 (func (param i32) (result i32)))
        (type $t1 (func (result i32)))
        (memory 1)
        (global $g0 (mut i32) (i32.const 0))
        (func $foo (type $t0) (param i32) (result i32)
            get_local 0
        ))
    "#;
    let wasm = wat2wasm(wat).unwrap();

    let (info, code_reader) = read_info::read_module(&wasm).unwrap();

    let (module, intrinsics) = code::parse_function_bodies(&info, code_reader).unwrap();

    let backend = backend::LLVMBackend::new(module, intrinsics);

    let func_ptr = backend.get_func(&info, LocalFuncIndex::new(0)).unwrap();

    println!("func_ptr: {:p}", func_ptr.as_ptr());

    unsafe {
        let func: unsafe extern "C" fn(*mut vm::Ctx, i32) -> i32 = transmute(func_ptr);
        let result = func(0 as _, 42);
        println!("result: {}", result);
    }
}
