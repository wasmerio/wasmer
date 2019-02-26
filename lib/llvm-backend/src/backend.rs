use crate::intrinsics::Intrinsics;
use dlopen::symbor::Library;
use inkwell::{
    module::Module,
    targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine},
    OptimizationLevel,
};
use std::{io::Write, ptr::NonNull};
use tempfile::NamedTempFile;
use wasmer_runtime_core::{
    backend::FuncResolver, module::ModuleInner, structures::TypedIndex, types::LocalFuncIndex, vm,
};

pub struct LLVMBackend {
    tempfile: NamedTempFile,
    library: Library,
}

impl LLVMBackend {
    pub fn new(module: Module, intrinsics: Intrinsics) -> Self {
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
        let target_machine = target
            .create_target_machine(
                &triple,
                &TargetMachine::get_host_cpu_name().to_string(),
                &TargetMachine::get_host_cpu_features().to_string(),
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .unwrap();

        let memory_buffer = target_machine
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        let mut tempfile = NamedTempFile::new().unwrap();
        tempfile.write_all(memory_buffer.as_slice()).unwrap();
        tempfile.flush().unwrap();

        let library = Library::open(tempfile.path()).unwrap();

        Self { tempfile, library }
    }
}

impl FuncResolver for LLVMBackend {
    fn get(
        &self,
        module: &ModuleInner,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        let index = module.info.imported_functions.len() + local_func_index.index();
        let name = if cfg!(macos) {
            format!("_fn{}", index)
        } else {
            format!("fn{}", index)
        };

        unsafe {
            self.library
                .symbol::<NonNull<vm::Func>>(&name)
                .ok()
                .map(|symbol| *symbol)
        }
    }
}
