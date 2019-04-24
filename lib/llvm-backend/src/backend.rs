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
    any::Any,
    ffi::{c_void, CString},
    mem,
    ops::Deref,
    ptr::{self, NonNull},
    slice, str,
    sync::{Arc, Once},
};
use wasmer_runtime_core::{
    backend::{
        sys::{Memory, Protect},
        CacheGen, RunnableModule,
    },
    cache::Error as CacheError,
    module::ModuleInfo,
    structures::TypedIndex,
    typed_func::{Wasm, WasmTrapInfo},
    types::{LocalFuncIndex, SigIndex},
    vm, vmcalls,
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

    lookup_vm_symbol: extern "C" fn(*const c_char, usize) -> *const vm::Func,
    visit_fde: extern "C" fn(*mut u8, usize, extern "C" fn(*mut u8)),
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

    fn throw_trap(ty: i32);

    /// This should be the same as spliting up the fat pointer into two arguments,
    /// but this is cleaner, I think?
    #[cfg_attr(nightly, unwind(allowed))]
    #[allow(improper_ctypes)]
    fn throw_any(data: *mut dyn Any) -> !;

    #[allow(improper_ctypes)]
    fn invoke_trampoline(
        trampoline: unsafe extern "C" fn(*mut vm::Ctx, NonNull<vm::Func>, *const u64, *mut u64),
        vmctx_ptr: *mut vm::Ctx,
        func_ptr: NonNull<vm::Func>,
        params: *const u64,
        results: *mut u64,
        trap_out: *mut WasmTrapInfo,
        user_error: *mut Option<Box<dyn Any>>,
        invoke_env: Option<NonNull<c_void>>,
    ) -> bool;
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

    extern "C" fn lookup_vm_symbol(name_ptr: *const c_char, length: usize) -> *const vm::Func {
        #[cfg(target_os = "macos")]
        macro_rules! fn_name {
            ($s:literal) => {
                concat!("_", $s)
            };
        }

        #[cfg(not(target_os = "macos"))]
        macro_rules! fn_name {
            ($s:literal) => {
                $s
            };
        }

        let name_slice = unsafe { slice::from_raw_parts(name_ptr as *const u8, length) };
        let name = str::from_utf8(name_slice).unwrap();

        match name {
            fn_name!("vm.memory.grow.dynamic.local") => vmcalls::local_dynamic_memory_grow as _,
            fn_name!("vm.memory.size.dynamic.local") => vmcalls::local_dynamic_memory_size as _,
            fn_name!("vm.memory.grow.static.local") => vmcalls::local_static_memory_grow as _,
            fn_name!("vm.memory.size.static.local") => vmcalls::local_static_memory_size as _,

            fn_name!("vm.exception.trap") => throw_trap as _,

            _ => ptr::null(),
        }
    }

    extern "C" fn visit_fde(fde: *mut u8, size: usize, visitor: extern "C" fn(*mut u8)) {
        unsafe {
            crate::platform::visit_fde(fde, size, visitor);
        }
    }

    Callbacks {
        alloc_memory,
        protect_memory,
        dealloc_memory,
        lookup_vm_symbol,
        visit_fde,
    }
}

pub enum Buffer {
    LlvmMemory(MemoryBuffer),
    Memory(Memory),
}

impl Deref for Buffer {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Buffer::LlvmMemory(mem_buffer) => mem_buffer.as_slice(),
            Buffer::Memory(memory) => unsafe { memory.as_slice() },
        }
    }
}

unsafe impl Send for LLVMBackend {}
unsafe impl Sync for LLVMBackend {}

pub struct LLVMBackend {
    module: *mut LLVMModule,
    #[allow(dead_code)]
    buffer: Arc<Buffer>,
}

impl LLVMBackend {
    pub fn new(module: Module, _intrinsics: Intrinsics) -> (Self, LLVMCache) {
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
                OptimizationLevel::Aggressive,
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

        static SIGNAL_HANDLER_INSTALLED: Once = Once::new();

        SIGNAL_HANDLER_INSTALLED.call_once(|| unsafe {
            crate::platform::install_signal_handler();
        });

        if res != LLVMResult::OK {
            panic!("failed to load object")
        }

        let buffer = Arc::new(Buffer::LlvmMemory(memory_buffer));

        (
            Self {
                module,
                buffer: Arc::clone(&buffer),
            },
            LLVMCache { buffer },
        )
    }

    pub unsafe fn from_buffer(memory: Memory) -> Result<(Self, LLVMCache), String> {
        let callbacks = get_callbacks();
        let mut module: *mut LLVMModule = ptr::null_mut();

        let slice = memory.as_slice();

        let res = module_load(slice.as_ptr(), slice.len(), callbacks, &mut module);

        if res != LLVMResult::OK {
            return Err("failed to load object".to_string());
        }

        static SIGNAL_HANDLER_INSTALLED: Once = Once::new();

        SIGNAL_HANDLER_INSTALLED.call_once(|| {
            crate::platform::install_signal_handler();
        });

        let buffer = Arc::new(Buffer::Memory(memory));

        Ok((
            Self {
                module,
                buffer: Arc::clone(&buffer),
            },
            LLVMCache { buffer },
        ))
    }
}

impl Drop for LLVMBackend {
    fn drop(&mut self) {
        unsafe { module_delete(self.module) }
    }
}

impl RunnableModule for LLVMBackend {
    fn get_func(
        &self,
        info: &ModuleInfo,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        let index = info.imported_functions.len() + local_func_index.index();
        let name = if cfg!(target_os = "macos") {
            format!("_fn{}", index)
        } else {
            format!("fn{}", index)
        };

        let c_str = CString::new(name).ok()?;
        let ptr = unsafe { get_func_symbol(self.module, c_str.as_ptr()) };

        NonNull::new(ptr as _)
    }

    fn get_trampoline(&self, _: &ModuleInfo, sig_index: SigIndex) -> Option<Wasm> {
        let trampoline: unsafe extern "C" fn(
            *mut vm::Ctx,
            NonNull<vm::Func>,
            *const u64,
            *mut u64,
        ) = unsafe {
            let name = if cfg!(target_os = "macos") {
                format!("_trmp{}", sig_index.index())
            } else {
                format!("trmp{}", sig_index.index())
            };

            let c_str = CString::new(name).unwrap();
            let symbol = get_func_symbol(self.module, c_str.as_ptr());
            assert!(!symbol.is_null());

            mem::transmute(symbol)
        };

        Some(unsafe { Wasm::from_raw_parts(trampoline, invoke_trampoline, None) })
    }

    unsafe fn do_early_trap(&self, data: Box<dyn Any>) -> ! {
        throw_any(Box::leak(data))
    }
}

unsafe impl Send for LLVMCache {}
unsafe impl Sync for LLVMCache {}

pub struct LLVMCache {
    buffer: Arc<Buffer>,
}

impl CacheGen for LLVMCache {
    fn generate_cache(&self) -> Result<(Box<[u8]>, Memory), CacheError> {
        let mut memory = Memory::with_size_protect(self.buffer.len(), Protect::ReadWrite)
            .map_err(CacheError::SerializeError)?;

        let buffer = self.buffer.deref();

        unsafe {
            memory.as_slice_mut()[..buffer.len()].copy_from_slice(buffer);
        }

        Ok(([].as_ref().into(), memory))
    }
}

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
