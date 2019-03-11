//! This module will take compile requests and run them until done.

use lazy_static::lazy_static;
use rayon;
use wasmer_runtime_core::types::LocalFuncIndex;

pub enum CompileRequest {
    Cranelift { func_index: LocalFuncIndex },
    LLVM { func_index: LocalFuncIndex },
}

pub fn submit_request(request: CompileRequest) {
    rayon::spawn(move || {});
}
