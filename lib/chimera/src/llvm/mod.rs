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
use wasmparser::{self, WasmDecoder};

mod backend;
mod compile;
mod intrinsics;
mod platform;
mod read_info;
mod stackmap;
mod state;

pub use self::compile::{compile_function, Opt};

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
        validate(wasm)?;

        // let (info, code_reader) = read_info::read_module(wasm).unwrap();
        unimplemented!()
        // let (module, intrinsics) = compile::parse_function_bodies(&info, code_reader).unwrap();

        // let (backend, protected_caller) = backend::LLVMBackend::new(module, intrinsics);

        // // Create placeholder values here.
        // let cache_gen = {
        //     use wasmer_runtime_core::backend::{
        //         sys::Memory, CacheGen, ProtectedCaller, UserTrapper,
        //     };
        //     use wasmer_runtime_core::cache::Error as CacheError;
        //     use wasmer_runtime_core::error::RuntimeResult;
        //     use wasmer_runtime_core::module::ModuleInfo;
        //     use wasmer_runtime_core::types::{FuncIndex, Value};
        //     use wasmer_runtime_core::vm;
        //     struct Placeholder;
        //     impl CacheGen for Placeholder {
        //         fn generate_cache(
        //             &self,
        //             module: &ModuleInner,
        //         ) -> Result<(Box<ModuleInfo>, Box<[u8]>, Memory), CacheError> {
        //             unimplemented!()
        //         }
        //     }

        //     Box::new(Placeholder)
        // };

        // Ok(ModuleInner {
        //     func_resolver: Box::new(backend),
        //     protected_caller: Box::new(protected_caller),
        //     cache_gen,

        //     info,
        // })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "disasm")]
    unsafe fn disass_ptr(ptr: *const u8, size: usize, inst_count: usize) {
        use capstone::arch::BuildsCapstone;
        let mut cs = capstone::Capstone::new() // Call builder-pattern
            .x86() // X86 architecture
            .mode(capstone::arch::x86::ArchMode::Mode64) // 64-bit mode
            .detail(true) // Generate extra instruction details
            .build()
            .expect("Failed to create Capstone object");

        // Get disassembled instructions
        let insns = cs
            .disasm_count(
                std::slice::from_raw_parts(ptr, size),
                ptr as u64,
                inst_count,
            )
            .expect("Failed to disassemble");

        println!("count = {}", insns.len());
        for insn in insns.iter() {
            println!(
                "0x{:x}: {:6} {}",
                insn.address(),
                insn.mnemonic().unwrap_or(""),
                insn.op_str().unwrap_or("")
            );
        }
    }
    #[cfg(not(feature = "disasm"))]
    unsafe fn disass_ptr(_ptr: *const u8, _size: usize, _inst_count: usize) {}

    #[test]
    fn read_module() {
        use crate::alloc_pool::AllocPool;
        use std::mem::transmute;
        use wabt::wat2wasm;
        use wasmer_runtime_core::{structures::TypedIndex, types::LocalFuncIndex, vm, vmcalls};
        // let wasm = include_bytes!("../../spectests/examples/simple/simple.wasm") as &[u8];
        let wat = r#"
            (module
            (type $t0 (func (param i32) (result i32)))
            (type $t1 (func (result i32)))
            (memory 1 1)
            (global $g0 (mut i32) (i32.const 0))
            (func $bar (type $t0) (param i32) (result i32)
                get_local 0
            )
            (func $foo (type $t0) (param i32) (result i32)
                get_local 0
                call $bar
            ))
        "#;
        let wasm = wat2wasm(wat).unwrap();

        let info_collection = crate::pipeline::InfoCollection::new(&wasm).unwrap();
        let (info, baseline_compile) = info_collection.run().unwrap();

        let pool = AllocPool::new();

        let codes = baseline_compile
            .run(|code_reader| compile::compile_module(&pool, &info, code_reader).unwrap());

        unsafe {
            let second_id = &codes[1];
            let ptr = pool.get(second_id).code_ptr().as_ptr();
            disass_ptr(ptr, 100, 5);
        }

        // println!("codes: {:?}", codes);

        // let (module, intrinsics) = code::parse_function_bodies(&info, code_reader).unwrap();

        // let (backend, _caller) = backend::LLVMBackend::new(module, intrinsics);

        // let func_ptr = backend.get_func(&info, LocalFuncIndex::new(1)).unwrap();

        // println!("func_ptr: {:p}", func_ptr.as_ptr());

        // unsafe {
        //     let func: unsafe extern "C" fn(*mut vm::Ctx, i32) -> i32 = transmute(func_ptr);
        //     let result = func(0 as _, 42);
        //     println!("result: {}", result);
        // }
    }
}
