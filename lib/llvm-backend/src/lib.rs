use wasmer_runtime_core::{
    backend::{Compiler, Token},
    error::CompileError,
    module::ModuleInner,
};
use inkwell::{
    execution_engine::JitFunction,
    targets::{TargetMachine, Target, RelocMode, CodeModel, InitializationConfig, FileType},
    OptimizationLevel,
};

mod code;
mod intrinsics;
mod read_info;
mod state;
// mod backend;

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
        let (_info, _code_reader) = read_info::read_module(wasm).unwrap();

        unimplemented!()
    }
}

#[test]
fn test_read_module() {
    use wasmer_runtime_core::vmcalls;
    use wabt::wat2wasm;
    // let wasm = include_bytes!("../../spectests/examples/simple/simple.wasm") as &[u8];
    let wat = r#"
        (module
        (type $t0 (func (param i32) (result i32)))
        (type $t1 (func (result i32)))
        (memory 1)
        (global $g0 (mut i32) (i32.const 0))
        (func $foo (type $t0) (param i32) (result i32)
            get_local 0
            call $foobar
            memory.grow
        )
        (func $foobar (type $t0)
            get_local 0
        )
        (func $bar (type $t0) (param i32) (result i32)
            get_local 0
            call $foo
        ))
    "#;
    let wasm = wat2wasm(wat).unwrap();

    let (info, code_reader) = read_info::read_module(&wasm).unwrap();

    let (module, intrinsics) = code::parse_function_bodies(&info, code_reader).unwrap();

    {
        Target::initialize_x86(&InitializationConfig {
            asm_parser: true,
            asm_printer: true,
            base: true,
            disassembler: true,
            info: true,
            machine_code: true,
        });
        let triple = TargetMachine::get_default_triple().to_string();
        let target = Target::from_triple(&triple).unwrap();
        let target_machine = target.create_target_machine(
            &triple,
            &TargetMachine::get_host_cpu_name().to_string(),
            &TargetMachine::get_host_cpu_features().to_string(),
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        ).unwrap();

        let memory_buffer = target_machine.write_to_memory_buffer(&module, FileType::Object).unwrap();
        // std::fs::write("memory_buffer", memory_buffer.as_slice()).unwrap();
        let mem_buf_slice = memory_buffer.as_slice();
        
        let macho = goblin::mach::MachO::parse(mem_buf_slice, 0).unwrap();
        let symbols = macho.symbols.as_ref().unwrap();
        let relocations = macho.relocations().unwrap();
        for (_, reloc_iter, section) in relocations.into_iter() {
            println!("section: {:#?}", section);
            for reloc_info in reloc_iter {
                let reloc_info = reloc_info.unwrap();
                println!("\treloc_info: {:#?}", reloc_info);
                println!("\tsymbol: {:#?}", symbols.get(reloc_info.r_symbolnum()).unwrap());
            }
        }
    }
        

    let exec_engine = module.create_jit_execution_engine(OptimizationLevel::Default).unwrap();

    exec_engine.add_global_mapping(&intrinsics.memory_grow_dynamic_local, vmcalls::local_dynamic_memory_grow as usize);
    exec_engine.add_global_mapping(&intrinsics.memory_grow_static_local, vmcalls::local_static_memory_grow as usize);
    exec_engine.add_global_mapping(&intrinsics.memory_grow_dynamic_import, vmcalls::imported_dynamic_memory_grow as usize);
    exec_engine.add_global_mapping(&intrinsics.memory_grow_static_import, vmcalls::imported_static_memory_grow as usize);
    exec_engine.add_global_mapping(&intrinsics.memory_size_dynamic_local, vmcalls::local_dynamic_memory_size as usize);
    exec_engine.add_global_mapping(&intrinsics.memory_size_static_local, vmcalls::local_static_memory_size as usize);
    exec_engine.add_global_mapping(&intrinsics.memory_size_dynamic_import, vmcalls::imported_dynamic_memory_size as usize);
    exec_engine.add_global_mapping(&intrinsics.memory_size_static_import, vmcalls::imported_static_memory_size as usize);

    // unsafe {
    //     let func: JitFunction<unsafe extern fn(*mut u8, i32) -> i32> = exec_engine.get_function("fn0").unwrap();
    //     let result = func.call(0 as _, 0);
    //     println!("result: {}", result);
    // }
}
