use inkwell::{
    module::Module,
    execution_engine::{ExecutionEngine, JitFunction},
};
use crate::intrinsics::Intrinsics;
use std::ptr::NonNull;
use wasmer_runtime_core::{
    module::ModuleInner,
    types::LocalFuncIndex,
    structures::TypedIndex,
    backend::{FuncResolver, vm},
};

pub struct LLVMBackend {
    exec_engine: ExecutionEngine,
}

impl LLVMBackend {
    pub fn new(module: Module, intrinsics: Intrinsics) -> Self {
        let exec_engine = module.create_jit_execution_engine(OptimizationLevel::Default).unwrap();

        exec_engine.add_global_mapping(&intrinsics.memory_grow_dynamic_local, vmcalls::local_dynamic_memory_grow as usize);
        exec_engine.add_global_mapping(&intrinsics.memory_grow_static_local, vmcalls::local_static_memory_grow as usize);
        exec_engine.add_global_mapping(&intrinsics.memory_grow_dynamic_import, vmcalls::imported_dynamic_memory_grow as usize);
        exec_engine.add_global_mapping(&intrinsics.memory_grow_static_import, vmcalls::imported_static_memory_grow as usize);
        exec_engine.add_global_mapping(&intrinsics.memory_size_dynamic_local, vmcalls::local_dynamic_memory_size as usize);
        exec_engine.add_global_mapping(&intrinsics.memory_size_static_local, vmcalls::local_static_memory_size as usize);
        exec_engine.add_global_mapping(&intrinsics.memory_size_dynamic_import, vmcalls::imported_dynamic_memory_size as usize);
        exec_engine.add_global_mapping(&intrinsics.memory_size_static_import, vmcalls::imported_static_memory_size as usize);

        Self { exec_engine }
    }
}

impl FuncResolver for LLVMBackend {
    fn get(&self, module: &ModuleInner, local_func_index: LocalFuncIndex) -> Option<NonNull<vm::Func>> {
        let index = module.info.imported_functions.len() + local_func_index.index();
        let name = format!("fn{}", index);
        
        unsafe {
            let func: JitFunction<unsafe extern fn()> = self.exec_engine.get_function(&name).ok()?;
            
        }
    }
}