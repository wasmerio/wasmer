use libc::c_char;

use wasmer_runtime_core::vm;

#[repr(C)]
pub struct LLVMModule {
    _private: [u8; 0],
}

#[allow(non_camel_case_types, dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub enum MemProtect {
    NONE,
    READ,
    READ_WRITE,
    READ_EXECUTE,
}

#[allow(non_camel_case_types, dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub enum LLVMResult {
    OK,
    ALLOCATE_FAILURE,
    PROTECT_FAILURE,
    DEALLOC_FAILURE,
    OBJECT_LOAD_FAILURE,
}

#[repr(C)]
pub struct Callbacks {
    pub alloc_memory: extern "C" fn(usize, MemProtect, &mut *mut u8, &mut usize) -> LLVMResult,
    pub protect_memory: extern "C" fn(*mut u8, usize, MemProtect) -> LLVMResult,
    pub dealloc_memory: extern "C" fn(*mut u8, usize) -> LLVMResult,

    pub lookup_vm_symbol: extern "C" fn(*const c_char, usize) -> *const vm::Func,
    pub visit_fde: extern "C" fn(*mut u8, usize, extern "C" fn(*mut u8)),
}
