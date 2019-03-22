use super::intrinsics::Intrinsics;
use super::platform;
use crate::{
    code::{CallOffset, Code},
    pool::{AllocId, PagePool},
    utils::lazy::Lazy,
};
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
    alloc::{alloc_zeroed, dealloc as _dealloc, Layout},
    any::Any,
    ffi::CString,
    mem,
    ptr::{self, NonNull},
    slice, str,
    sync::Once,
};
use wasmer_runtime_core::{
    backend::{FuncResolver, ProtectedCaller, Token, UserTrapper},
    error::{RuntimeError, RuntimeResult},
    export::Context,
    module::{ModuleInfo, ModuleInner},
    structures::TypedIndex,
    types::{
        FuncIndex, FuncSig, LocalFuncIndex, LocalOrImport, MemoryIndex, SigIndex, TableIndex, Type,
        Value,
    },
    vm::{self, ImportBacking},
    vmcalls,
};

#[repr(C)]
struct LLVMFunction {
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
enum WasmTrapType {
    Unreachable = 0,
    IncorrectCallIndirectSignature = 1,
    MemoryOutOfBounds = 2,
    CallIndirectOOB = 3,
    IllegalArithmetic = 4,
    Unknown,
}

#[repr(C)]
struct Callbacks {
    alloc: extern "C" fn(usize, usize) -> Option<NonNull<u8>>,
    dealloc: extern "C" fn(NonNull<u8>, usize, usize),
    create_code: extern "C" fn(&PagePool, u32, &mut AllocId<Code>) -> Option<NonNull<u8>>,

    lookup_vm_symbol: extern "C" fn(*const c_char, usize) -> *const vm::Func,
    visit_fde: extern "C" fn(*mut u8, usize, extern "C" fn(*mut u8)),
}

extern "C" {
    fn function_load(
        mem_ptr: *const u8,
        mem_size: usize,
        callbacks: Callbacks,
        pool: &PagePool,
        func_out: &mut *mut LLVMFunction,
        code_id_out: &mut AllocId<Code>,
    ) -> LLVMResult;
    fn get_stackmap(func: *mut LLVMFunction, size_out: &mut usize) -> Option<NonNull<u8>>;
    fn function_delete(func: *mut LLVMFunction);

    fn throw_trap(ty: i32);

    fn invoke_trampoline(
        trampoline: unsafe extern "C" fn(*mut vm::Ctx, *const vm::Func, *const u64, *mut u64),
        vmctx_ptr: *mut vm::Ctx,
        func_ptr: *const vm::Func,
        params: *const u64,
        results: *mut u64,
        trap_out: *mut WasmTrapType,
    ) -> bool;
}

fn get_callbacks() -> Callbacks {
    extern "C" fn alloc(size: usize, align: usize) -> Option<NonNull<u8>> {
        unsafe {
            let layout = Layout::from_size_align_unchecked(size, align);
            NonNull::new(alloc_zeroed(layout))
        }
    }

    extern "C" fn dealloc(ptr: NonNull<u8>, size: usize, align: usize) {
        unsafe {
            let layout = Layout::from_size_align_unchecked(size, align);
            _dealloc(ptr.as_ptr(), layout);
        }
    }

    extern "C" fn create_code(
        pool: &PagePool,
        code_size: u32,
        offset_out: &mut AllocId<Code>,
    ) -> Option<NonNull<u8>> {
        let code_id = Code::new(pool, code_size, ()).ok()?;
        let code = pool.get(&code_id);
        let ptr = code.code_ptr();
        *offset_out = code_id;
        Some(ptr)
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
            platform::visit_fde(fde, size, visitor);
        }
    }

    Callbacks {
        alloc,
        dealloc,
        create_code,
        lookup_vm_symbol,
        visit_fde,
    }
}

unsafe impl Send for Function {}
unsafe impl Sync for Function {}

pub struct Function {
    func: *mut LLVMFunction,
    #[allow(dead_code)]
    memory_buffer: MemoryBuffer,
}

impl Function {
    pub fn new(pool: &PagePool, module: Module, intrinsics: Intrinsics) -> AllocId<Code> {
        unsafe impl Sync for SyncTargetMachine {}
        /// I'm going to assume that TargetMachine is actually threadsafe.
        /// It might not be, but there's really no way of knowing.
        struct SyncTargetMachine(TargetMachine);

        static TARGET: Lazy<SyncTargetMachine> = Lazy::new(|| {
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
            SyncTargetMachine(
                target
                    .create_target_machine(
                        &triple,
                        &TargetMachine::get_host_cpu_name().to_string(),
                        &TargetMachine::get_host_cpu_features().to_string(),
                        OptimizationLevel::Aggressive,
                        RelocMode::PIC,
                        CodeModel::Default,
                    )
                    .unwrap(),
            )
        });

        let memory_buffer = TARGET
            .0
            .write_to_memory_buffer(&module, FileType::Object)
            .unwrap();

        let mem_buf_slice = memory_buffer.as_slice();

        let callbacks = get_callbacks();
        let mut llvm_function: *mut LLVMFunction = ptr::null_mut();
        let mut alloc_id = unsafe { std::mem::zeroed() };

        let res = unsafe {
            function_load(
                mem_buf_slice.as_ptr(),
                mem_buf_slice.len(),
                callbacks,
                pool,
                &mut llvm_function,
                &mut alloc_id,
            )
        };
        assert!(res == LLVMResult::OK);

        let mut code = pool.get_mut(&mut alloc_id);

        if let (size, Some(stackmap_ptr)) = unsafe {
            let mut size = 0;
            (size, get_stackmap(llvm_function, &mut size))
        } {
            use super::stackmap::{StackMapRecord, Stackmap};
            let stackmap_slice = unsafe { slice::from_raw_parts(stackmap_ptr.as_ptr(), size) };
            let stackmap = Stackmap::parse(stackmap_slice).expect("unable to parse stackmap");

            code.call_offsets = stackmap
                .stack_map_records
                .iter()
                .map(
                    |&StackMapRecord {
                         patchpoint_id,
                         inst_offset,
                         ..
                     }| {
                        let func_index = LocalFuncIndex::new(patchpoint_id as _);
                        CallOffset {
                            func_index,
                            offset: inst_offset,
                        }
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
        }

        // {
        //     println!("checking for stackmap");
        //     let mut size = 0;
        //     if let Some(stackmap_ptr) = unsafe { get_stackmap(module, &mut size) } {
        //         use super::stackmap::Stackmap;

        //         println!("size: {}", size);

        //         let stackmap_slice = unsafe { slice::from_raw_parts(stackmap_ptr.as_ptr(), size) };
        //         let stackmap = Stackmap::parse(stackmap_slice).unwrap();
        //         println!("{:#?}", stackmap);
        //     } else {
        //         println!("no stackmap");
        //     }
        // }

        let function = Self {
            func: llvm_function,
            memory_buffer,
        };
        code.keep_alive = Box::new(function);

        alloc_id
    }
}

impl Drop for Function {
    fn drop(&mut self) {
        unsafe { function_delete(self.func) }
    }
}

// impl FuncResolver for Function {
//     fn get(
//         &self,
//         module: &ModuleInner,
//         local_func_index: LocalFuncIndex,
//     ) -> Option<NonNull<vm::Func>> {
//         self.get_func(&module.info, local_func_index)
//     }
// }

// struct Placeholder;

// unsafe impl Send for LLVMProtectedCaller {}
// unsafe impl Sync for LLVMProtectedCaller {}

// pub struct LLVMProtectedCaller {
//     module: *mut LLVMFunction,
// }

// impl ProtectedCaller for LLVMProtectedCaller {
//     fn call(
//         &self,
//         module: &ModuleInner,
//         func_index: FuncIndex,
//         params: &[Value],
//         import_backing: &ImportBacking,
//         vmctx: *mut vm::Ctx,
//         _: Token,
//     ) -> RuntimeResult<Vec<Value>> {
//         let (func_ptr, ctx, signature, sig_index) =
//             get_func_from_index(&module, import_backing, func_index);

//         let vmctx_ptr = match ctx {
//             Context::External(external_vmctx) => external_vmctx,
//             Context::Internal => vmctx,
//         };

//         assert!(
//             signature.returns().len() <= 1,
//             "multi-value returns not yet supported"
//         );

//         assert!(
//             signature.check_param_value_types(params),
//             "incorrect signature"
//         );

//         let param_vec: Vec<u64> = params
//             .iter()
//             .map(|val| match val {
//                 Value::I32(x) => *x as u64,
//                 Value::I64(x) => *x as u64,
//                 Value::F32(x) => x.to_bits() as u64,
//                 Value::F64(x) => x.to_bits(),
//             })
//             .collect();

//         let mut return_vec = vec![0; signature.returns().len()];

//         let trampoline: unsafe extern "C" fn(*mut vm::Ctx, *const vm::Func, *const u64, *mut u64) = unsafe {
//             let name = if cfg!(target_os = "macos") {
//                 format!("_trmp{}", sig_index.index())
//             } else {
//                 format!("trmp{}", sig_index.index())
//             };

//             let c_str = CString::new(name).unwrap();
//             let symbol = get_func_symbol(self.module, c_str.as_ptr());
//             assert!(!symbol.is_null());

//             mem::transmute(symbol)
//         };

//         let mut trap_out = WasmTrapType::Unknown;

//         // Here we go.
//         let success = unsafe {
//             invoke_trampoline(
//                 trampoline,
//                 vmctx_ptr,
//                 func_ptr,
//                 param_vec.as_ptr(),
//                 return_vec.as_mut_ptr(),
//                 &mut trap_out,
//             )
//         };

//         if success {
//             Ok(return_vec
//                 .iter()
//                 .zip(signature.returns().iter())
//                 .map(|(&x, ty)| match ty {
//                     Type::I32 => Value::I32(x as i32),
//                     Type::I64 => Value::I64(x as i64),
//                     Type::F32 => Value::F32(f32::from_bits(x as u32)),
//                     Type::F64 => Value::F64(f64::from_bits(x as u64)),
//                 })
//                 .collect())
//         } else {
//             Err(match trap_out {
//                 WasmTrapType::Unreachable => RuntimeError::Trap {
//                     msg: "unreachable".into(),
//                 },
//                 WasmTrapType::IncorrectCallIndirectSignature => RuntimeError::Trap {
//                     msg: "uncorrect call_indirect signature".into(),
//                 },
//                 WasmTrapType::MemoryOutOfBounds => RuntimeError::Trap {
//                     msg: "memory out-of-bounds access".into(),
//                 },
//                 WasmTrapType::CallIndirectOOB => RuntimeError::Trap {
//                     msg: "call_indirect out-of-bounds".into(),
//                 },
//                 WasmTrapType::IllegalArithmetic => RuntimeError::Trap {
//                     msg: "illegal arithmetic operation".into(),
//                 },
//                 WasmTrapType::Unknown => RuntimeError::Trap {
//                     msg: "unknown trap".into(),
//                 },
//             })
//         }
//     }

//     fn get_early_trapper(&self) -> Box<dyn UserTrapper> {
//         Box::new(Placeholder)
//     }
// }

// impl UserTrapper for Placeholder {
//     unsafe fn do_early_trap(&self, _data: Box<dyn Any>) -> ! {
//         unimplemented!("do early trap")
//     }
// }

// fn get_func_from_index<'a>(
//     module: &'a ModuleInner,
//     import_backing: &ImportBacking,
//     func_index: FuncIndex,
// ) -> (*const vm::Func, Context, &'a FuncSig, SigIndex) {
//     let sig_index = *module
//         .info
//         .func_assoc
//         .get(func_index)
//         .expect("broken invariant, incorrect func index");

//     let (func_ptr, ctx) = match func_index.local_or_import(&module.info) {
//         LocalOrImport::Local(local_func_index) => (
//             module
//                 .func_resolver
//                 .get(&module, local_func_index)
//                 .expect("broken invariant, func resolver not synced with module.exports")
//                 .cast()
//                 .as_ptr() as *const _,
//             Context::Internal,
//         ),
//         LocalOrImport::Import(imported_func_index) => {
//             let imported_func = import_backing.imported_func(imported_func_index);
//             (
//                 imported_func.func as *const _,
//                 Context::External(imported_func.vmctx),
//             )
//         }
//     };

//     let signature = &module.info.signatures[sig_index];

//     (func_ptr, ctx, signature, sig_index)
// }

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
