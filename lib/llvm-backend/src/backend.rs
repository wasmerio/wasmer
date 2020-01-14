use super::stackmap::StackmapRegistry;
use crate::{
    intrinsics::Intrinsics,
    structs::{Callbacks, LLVMModule, LLVMResult, MemProtect},
    LLVMCallbacks,
};
use inkwell::{
    memory_buffer::MemoryBuffer,
    module::Module,
    targets::{FileType, TargetMachine},
};
use libc::c_char;
use std::{
    any::Any,
    cell::RefCell,
    ffi::{c_void, CString},
    mem,
    ops::Deref,
    ptr::{self, NonNull},
    rc::Rc,
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
    state::ModuleStateMap,
    structures::TypedIndex,
    typed_func::{Trampoline, Wasm, WasmTrapInfo},
    types::{LocalFuncIndex, SigIndex},
    vm, vmcalls,
};

extern "C" {
    fn module_load(
        mem_ptr: *const u8,
        mem_size: usize,
        callbacks: Callbacks,
        module_out: &mut *mut LLVMModule,
    ) -> LLVMResult;
    fn module_delete(module: *mut LLVMModule);
    fn get_func_symbol(module: *mut LLVMModule, name: *const c_char) -> *const vm::Func;
    fn llvm_backend_get_stack_map_ptr(module: *const LLVMModule) -> *const u8;
    fn llvm_backend_get_stack_map_size(module: *const LLVMModule) -> usize;
    fn llvm_backend_get_code_ptr(module: *const LLVMModule) -> *const u8;
    fn llvm_backend_get_code_size(module: *const LLVMModule) -> usize;

    fn throw_trap(ty: i32) -> !;
    fn throw_breakpoint(ty: i64) -> !;

    /// This should be the same as spliting up the fat pointer into two arguments,
    /// but this is cleaner, I think?
    #[cfg_attr(nightly, unwind(allowed))]
    #[allow(improper_ctypes)]
    fn throw_any(data: *mut dyn Any) -> !;

    #[allow(improper_ctypes)]
    fn invoke_trampoline(
        trampoline: Trampoline,
        vmctx_ptr: *mut vm::Ctx,
        func_ptr: NonNull<vm::Func>,
        params: *const u64,
        results: *mut u64,
        trap_out: *mut WasmTrapInfo,
        user_error: *mut Option<Box<dyn Any + Send>>,
        invoke_env: Option<NonNull<c_void>>,
    ) -> bool;
}

static SIGNAL_HANDLER_INSTALLED: Once = Once::new();

fn get_callbacks() -> Callbacks {
    extern "C" fn alloc_memory(
        size: usize,
        protect: MemProtect,
        ptr_out: &mut *mut u8,
        size_out: &mut usize,
    ) -> LLVMResult {
        unsafe { crate::platform::alloc_memory(size, protect, ptr_out, size_out) }
    }

    extern "C" fn protect_memory(ptr: *mut u8, size: usize, protect: MemProtect) -> LLVMResult {
        unsafe { crate::platform::protect_memory(ptr, size, protect) }
    }

    extern "C" fn dealloc_memory(ptr: *mut u8, size: usize) -> LLVMResult {
        unsafe { crate::platform::dealloc_memory(ptr, size) }
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

            fn_name!("vm.memory.grow.dynamic.import") => vmcalls::imported_dynamic_memory_grow as _,
            fn_name!("vm.memory.size.dynamic.import") => vmcalls::imported_dynamic_memory_size as _,
            fn_name!("vm.memory.grow.static.import") => vmcalls::imported_static_memory_grow as _,
            fn_name!("vm.memory.size.static.import") => vmcalls::imported_static_memory_size as _,

            fn_name!("vm.exception.trap") => throw_trap as _,
            fn_name!("vm.breakpoint") => throw_breakpoint as _,

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
    msm: Option<ModuleStateMap>,
    local_func_id_to_offset: Vec<usize>,
}

impl LLVMBackend {
    pub fn new(
        module: Rc<RefCell<Module>>,
        _intrinsics: Intrinsics,
        _stackmaps: &StackmapRegistry,
        _module_info: &ModuleInfo,
        target_machine: &TargetMachine,
        llvm_callbacks: &Option<Rc<RefCell<dyn LLVMCallbacks>>>,
    ) -> (Self, LLVMCache) {
        let memory_buffer = target_machine
            .write_to_memory_buffer(&module.borrow_mut(), FileType::Object)
            .unwrap();

        if let Some(callbacks) = llvm_callbacks {
            callbacks
                .borrow_mut()
                .obj_memory_buffer_callback(&memory_buffer);
        }

        let callbacks = get_callbacks();
        let mut module: *mut LLVMModule = ptr::null_mut();

        let mem_buf_slice = memory_buffer.as_slice();
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

        let buffer = Arc::new(Buffer::LlvmMemory(memory_buffer));

        #[cfg(all(any(target_os = "linux", target_os = "macos"), target_arch = "x86_64"))]
        {
            use super::stackmap::{self, StkMapRecord, StkSizeRecord};
            use std::collections::BTreeMap;

            let stackmaps = _stackmaps;
            let module_info = _module_info;

            let raw_stackmap = unsafe {
                std::slice::from_raw_parts(
                    llvm_backend_get_stack_map_ptr(module),
                    llvm_backend_get_stack_map_size(module),
                )
            };
            if raw_stackmap.len() > 0 {
                let map = stackmap::StackMap::parse(raw_stackmap).unwrap();

                let (code_ptr, code_size) = unsafe {
                    (
                        llvm_backend_get_code_ptr(module),
                        llvm_backend_get_code_size(module),
                    )
                };
                let mut msm = ModuleStateMap {
                    local_functions: Default::default(),
                    total_size: code_size,
                };

                let num_local_functions =
                    module_info.func_assoc.len() - module_info.imported_functions.len();
                let mut local_func_id_to_addr: Vec<usize> = Vec::with_capacity(num_local_functions);

                // All local functions.
                for index in module_info.imported_functions.len()..module_info.func_assoc.len() {
                    let name = if cfg!(target_os = "macos") {
                        format!("_fn{}", index)
                    } else {
                        format!("fn{}", index)
                    };

                    let c_str = CString::new(name).unwrap();
                    let ptr = unsafe { get_func_symbol(module, c_str.as_ptr()) };

                    assert!(!ptr.is_null());
                    local_func_id_to_addr.push(ptr as usize);
                }

                let mut addr_to_size_record: BTreeMap<usize, &StkSizeRecord> = BTreeMap::new();

                for record in &map.stk_size_records {
                    addr_to_size_record.insert(record.function_address as usize, record);
                }

                let mut map_records: BTreeMap<usize, &StkMapRecord> = BTreeMap::new();

                for record in &map.stk_map_records {
                    map_records.insert(record.patchpoint_id as usize, record);
                }

                for ((start_id, start_entry), (end_id, end_entry)) in stackmaps
                    .entries
                    .iter()
                    .enumerate()
                    .step_by(2)
                    .zip(stackmaps.entries.iter().enumerate().skip(1).step_by(2))
                {
                    if let Some(map_record) = map_records.get(&start_id) {
                        assert_eq!(start_id, map_record.patchpoint_id as usize);
                        assert!(start_entry.is_start);
                        assert!(!end_entry.is_start);

                        let end_record = map_records.get(&end_id);

                        let addr = local_func_id_to_addr[start_entry.local_function_id];
                        let size_record = *addr_to_size_record
                            .get(&addr)
                            .expect("size_record not found");

                        start_entry.populate_msm(
                            module_info,
                            code_ptr as usize,
                            &map,
                            size_record,
                            map_record,
                            end_record.map(|x| (end_entry, *x)),
                            &mut msm,
                        );
                    } else {
                        // The record is optimized out.
                    }
                }

                let code_ptr = unsafe { llvm_backend_get_code_ptr(module) } as usize;
                let code_len = unsafe { llvm_backend_get_code_size(module) } as usize;

                let local_func_id_to_offset: Vec<usize> = local_func_id_to_addr
                    .iter()
                    .map(|&x| {
                        assert!(x >= code_ptr && x < code_ptr + code_len);
                        x - code_ptr
                    })
                    .collect();

                return (
                    Self {
                        module,
                        buffer: Arc::clone(&buffer),
                        msm: Some(msm),
                        local_func_id_to_offset,
                    },
                    LLVMCache { buffer },
                );
            }
        }

        // Stackmap is not supported on this platform, or this module contains no functions so no stackmaps.
        (
            Self {
                module,
                buffer: Arc::clone(&buffer),
                msm: None,
                local_func_id_to_offset: vec![],
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

        SIGNAL_HANDLER_INSTALLED.call_once(|| {
            crate::platform::install_signal_handler();
        });

        let buffer = Arc::new(Buffer::Memory(memory));

        Ok((
            Self {
                module,
                buffer: Arc::clone(&buffer),
                msm: None,
                local_func_id_to_offset: vec![],
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
        let trampoline: Trampoline = unsafe {
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

        SIGNAL_HANDLER_INSTALLED.call_once(|| unsafe {
            crate::platform::install_signal_handler();
        });

        Some(unsafe { Wasm::from_raw_parts(trampoline, invoke_trampoline, None) })
    }

    fn get_code(&self) -> Option<&[u8]> {
        Some(unsafe {
            std::slice::from_raw_parts(
                llvm_backend_get_code_ptr(self.module),
                llvm_backend_get_code_size(self.module),
            )
        })
    }

    fn get_local_function_offsets(&self) -> Option<Vec<usize>> {
        Some(self.local_func_id_to_offset.clone())
    }

    fn get_module_state_map(&self) -> Option<ModuleStateMap> {
        self.msm.clone()
    }

    unsafe fn do_early_trap(&self, data: Box<dyn Any + Send>) -> ! {
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
