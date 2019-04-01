#![cfg_attr(nightly, feature(unwind_attributes))]

use wasmer_runtime_core::{
    backend::{Compiler, CompilerConfig, Token},
    cache::{Artifact, Error as CacheError},
    error::CompileError,
    module::ModuleInner,
};
use wasmparser::{self, WasmDecoder};

mod backend;
mod code;
mod intrinsics;
mod platform;
mod read_info;
mod state;
mod trampolines;

pub struct LLVMCompiler {
    _private: (),
}

impl LLVMCompiler {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Compiler for LLVMCompiler {
    fn compile(
        &self,
        wasm: &[u8],
        compiler_config: CompilerConfig,
        _: Token,
    ) -> Result<ModuleInner, CompileError> {
        validate(wasm)?;

        let (info, code_reader) = read_info::read_module(wasm, compiler_config).unwrap();
        let (module, intrinsics) = code::parse_function_bodies(&info, code_reader).unwrap();

        let (backend, protected_caller) = backend::LLVMBackend::new(module, intrinsics);

        // Create placeholder values here.
        let cache_gen = {
            use wasmer_runtime_core::backend::{sys::Memory, CacheGen};
            use wasmer_runtime_core::cache::Error as CacheError;
            use wasmer_runtime_core::module::ModuleInfo;

            struct Placeholder;

            impl CacheGen for Placeholder {
                fn generate_cache(
                    &self,
                    _module: &ModuleInner,
                ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError> {
                    unimplemented!()
                }
            }

            Box::new(Placeholder)
        };

        Ok(ModuleInner {
            func_resolver: Box::new(backend),
            protected_caller: Box::new(protected_caller),
            cache_gen,

            info,
        })
    }

    unsafe fn from_cache(&self, _artifact: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        unimplemented!("the llvm backend doesn't support caching yet")
    }
}

fn validate(bytes: &[u8]) -> Result<(), CompileError> {
    let mut parser = wasmparser::ValidatingParser::new(
        bytes,
        Some(wasmparser::ValidatingParserConfig {
            operator_config: wasmparser::OperatorValidatorConfig {
                enable_threads: false,
                enable_reference_types: false,
                enable_simd: false,
                enable_bulk_memory: false,
            },
            mutable_global_imports: false,
        }),
    );

    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => break Ok(()),
            wasmparser::ParserState::Error(err) => Err(CompileError::ValidationError {
                msg: err.message.to_string(),
            })?,
            _ => {}
        }
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

    let (info, code_reader) = read_info::read_module(&wasm, Default::default()).unwrap();

    let (module, intrinsics) = code::parse_function_bodies(&info, code_reader).unwrap();

    let (backend, _caller) = backend::LLVMBackend::new(module, intrinsics);

    let func_ptr = backend.get_func(&info, LocalFuncIndex::new(0)).unwrap();

    println!("func_ptr: {:p}", func_ptr.as_ptr());

    unsafe {
        let func: unsafe extern "C" fn(*mut vm::Ctx, i32) -> i32 = transmute(func_ptr);
        let result = func(0 as _, 42);
        println!("result: {}", result);
    }
}
