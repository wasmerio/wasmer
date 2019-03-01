use crate::intrinsics::Intrinsics;
use inkwell::{
    memory_buffer::MemoryBuffer,
    module::Module,
    targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine},
    OptimizationLevel,
};
use libc::{
    c_char, mmap, mprotect, munmap, MAP_ANON, MAP_PRIVATE, PROT_EXEC, PROT_NONE, PROT_READ,
    PROT_WRITE,
};
use std::{
    ffi::CString,
    ptr::{self, NonNull},
};
use wasmer_runtime_core::{
    backend::FuncResolver,
    module::{ModuleInfo, ModuleInner},
    structures::TypedIndex,
    types::LocalFuncIndex,
    vm,
};

#[repr(C)]
struct LLVMModule {
    _private: [u8; 0],
}

#[allow(non_camel_case_types, dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
enum MemProtect {
    NONE,
    READ,
    READ_WRITE,
    READ_EXECUTE,
}

#[allow(non_camel_case_types, dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
enum LLVMResult {
    OK,
    ALLOCATE_FAILURE,
    PROTECT_FAILURE,
    DEALLOC_FAILURE,
    OBJECT_LOAD_FAILURE,
}

#[repr(C)]
struct Callbacks {
    alloc_memory: extern "C" fn(usize, MemProtect, &mut *mut u8, &mut usize) -> LLVMResult,
    protect_memory: extern "C" fn(*mut u8, usize, MemProtect) -> LLVMResult,
    dealloc_memory: extern "C" fn(*mut u8, usize) -> LLVMResult,

    lookup_vm_symbol: extern "C" fn(*const c_char) -> *const vm::Func,
}

extern "C" {
    fn module_load(
        mem_ptr: *const u8,
        mem_size: usize,
        callbacks: Callbacks,
        module_out: &mut *mut LLVMModule,
    ) -> LLVMResult;
    fn module_delete(module: *mut LLVMModule);
    fn get_func_symbol(module: *mut LLVMModule, name: *const c_char) -> *const vm::Func;
}

fn get_callbacks() -> Callbacks {
    fn round_up_to_page_size(size: usize) -> usize {
        (size + (4096 - 1)) & !(4096 - 1)
    }

    extern "C" fn alloc_memory(
        size: usize,
        protect: MemProtect,
        ptr_out: &mut *mut u8,
        size_out: &mut usize,
    ) -> LLVMResult {
        let size = round_up_to_page_size(size);
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                match protect {
                    MemProtect::NONE => PROT_NONE,
                    MemProtect::READ => PROT_READ,
                    MemProtect::READ_WRITE => PROT_READ | PROT_WRITE,
                    MemProtect::READ_EXECUTE => PROT_READ | PROT_EXEC,
                },
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if ptr as isize == -1 {
            return LLVMResult::ALLOCATE_FAILURE;
        }
        *ptr_out = ptr as _;
        *size_out = size;
        LLVMResult::OK
    }

    extern "C" fn protect_memory(ptr: *mut u8, size: usize, protect: MemProtect) -> LLVMResult {
        let res = unsafe {
            mprotect(
                ptr as _,
                round_up_to_page_size(size),
                match protect {
                    MemProtect::NONE => PROT_NONE,
                    MemProtect::READ => PROT_READ,
                    MemProtect::READ_WRITE => PROT_READ | PROT_WRITE,
                    MemProtect::READ_EXECUTE => PROT_READ | PROT_EXEC,
                },
            )
        };

        if res == 0 {
            LLVMResult::OK
        } else {
            LLVMResult::PROTECT_FAILURE
        }
    }

    extern "C" fn dealloc_memory(ptr: *mut u8, size: usize) -> LLVMResult {
        let res = unsafe { munmap(ptr as _, round_up_to_page_size(size)) };

        if res == 0 {
            LLVMResult::OK
        } else {
            LLVMResult::DEALLOC_FAILURE
        }
    }

    extern "C" fn lookup_vm_symbol(_name_ptr: *const c_char) -> *const vm::Func {
        ptr::null()
    }

    Callbacks {
        alloc_memory,
        protect_memory,
        dealloc_memory,
        lookup_vm_symbol,
    }
}

unsafe impl Send for LLVMBackend {}
unsafe impl Sync for LLVMBackend {}

pub struct LLVMBackend {
    module: *mut LLVMModule,
    #[allow(dead_code)]
    memory_buffer: MemoryBuffer,
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
        let mem_buf_slice = memory_buffer.as_slice();

        let callbacks = get_callbacks();
        let mut module: *mut LLVMModule = ptr::null_mut();

        let res = unsafe {
            module_load(
                mem_buf_slice.as_ptr(),
                mem_buf_slice.len(),
                callbacks,
                &mut module,
            )
        };

        if res != LLVMResult::OK {
            panic!("failed to load object")
        }

        Self {
            module,
            memory_buffer,
        }
    }

    pub fn get_func(
        &self,
        info: &ModuleInfo,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        let index = local_func_index.index();
        let name = if cfg!(target_os = "macos") {
            format!("_fn{}", index)
        } else {
            format!("fn{}", index)
        };

        let c_str = CString::new(name).ok()?;
        let ptr = unsafe { get_func_symbol(self.module, c_str.as_ptr()) };

        NonNull::new(ptr as _)
    }
}

impl Drop for LLVMBackend {
    fn drop(&mut self) {
        unsafe { module_delete(self.module) }
    }
}

impl FuncResolver for LLVMBackend {
    fn get(
        &self,
        module: &ModuleInner,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        self.get_func(&module.info, local_func_index)
    }
}

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
