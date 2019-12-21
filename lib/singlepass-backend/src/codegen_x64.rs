#![allow(clippy::forget_copy)] // Used by dynasm.
#![warn(unused_imports)]

use crate::emitter_x64::*;
use crate::machine::*;
use crate::protect_unix;
#[cfg(target_arch = "aarch64")]
use dynasmrt::aarch64::Assembler;
#[cfg(target_arch = "x86_64")]
use dynasmrt::x64::Assembler;
use dynasmrt::{AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi};
use smallvec::SmallVec;
use std::{
    any::Any,
    collections::{BTreeMap, HashMap},
    ffi::c_void,
    iter, mem,
    ptr::NonNull,
    slice,
    sync::{Arc, RwLock},
    usize,
};
use wasmer_runtime_core::{
    backend::{
        sys::{Memory, Protect},
        Architecture, Backend, CacheGen, CompilerConfig, InlineBreakpoint, InlineBreakpointType,
        MemoryBoundCheckMode, RunnableModule, Token,
    },
    cache::{Artifact, Error as CacheError},
    codegen::*,
    fault::raw::register_preservation_trampoline,
    loader::CodeMemory,
    memory::MemoryType,
    module::{ModuleInfo, ModuleInner},
    state::{
        x64::new_machine_state, x64::X64Register, FunctionStateMap, MachineState, MachineValue,
        ModuleStateMap, OffsetInfo, SuspendOffset, WasmAbstractValue,
    },
    structures::{Map, TypedIndex},
    typed_func::{Trampoline, Wasm, WasmTrapInfo},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, LocalOrImport, MemoryIndex, SigIndex,
        TableIndex, Type,
    },
    vm::{self, LocalGlobal, LocalTable, INTERNALS_SIZE},
    wasmparser::{MemoryImmediate, Operator, Type as WpType, TypeOrFuncType as WpTypeOrFuncType},
};

#[cfg(target_arch = "aarch64")]
#[allow(dead_code)]
static ARCH: Architecture = Architecture::Aarch64;
#[cfg(target_arch = "x86_64")]
#[allow(dead_code)]
static ARCH: Architecture = Architecture::X64;

/// Inline breakpoint size for x86-64.
pub const INLINE_BREAKPOINT_SIZE_X86_SINGLEPASS: usize = 7;

/// Inline breakpoint size for aarch64.
pub const INLINE_BREAKPOINT_SIZE_AARCH64_SINGLEPASS: usize = 12;

#[cfg(target_arch = "x86_64")]
lazy_static! {
    /// Performs a System V call to `target` with [stack_top..stack_base] as the argument list, from right to left.
    static ref CONSTRUCT_STACK_AND_CALL_WASM: unsafe extern "C" fn (stack_top: *const u64, stack_base: *const u64, ctx: *mut vm::Ctx, target: *const vm::Func) -> u64 = {
        let mut assembler = Assembler::new().unwrap();
        let offset = assembler.offset();
        dynasm!(
            assembler
            ; push r15
            ; push r14
            ; push r13
            ; push r12
            ; push r11
            ; push rbp
            ; mov rbp, rsp

            ; mov r15, rdi
            ; mov r14, rsi
            ; mov r13, rdx
            ; mov r12, rcx

            ; mov rdi, r13 // ctx

            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rsi, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rdx, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rcx, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov r8, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov r9, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rax, r14
            ; sub rax, r15
            ; sub rsp, rax
            ; sub rsp, 8
            ; mov rax, QWORD 0xfffffffffffffff0u64 as i64
            ; and rsp, rax
            ; mov rax, rsp
            ; loop_begin:
            ; mov r11, [r14]
            ; mov [rax], r11
            ; sub r14, 8
            ; add rax, 8
            ; cmp r14, r15
            ; jb >stack_ready
            ; jmp <loop_begin

            ; stack_ready:
            ; mov rax, QWORD 0xfffffffffffffff0u64 as i64
            ; and rsp, rax
            ; call r12

            ; mov rsp, rbp
            ; pop rbp
            ; pop r11
            ; pop r12
            ; pop r13
            ; pop r14
            ; pop r15
            ; ret
        );
        let buf = assembler.finalize().unwrap();
        let ret = unsafe { mem::transmute(buf.ptr(offset)) };
        mem::forget(buf);
        ret
    };
}

#[cfg(target_arch = "aarch64")]
#[repr(C)]
#[allow(dead_code)]
struct CallCtx {
    ctx: *mut vm::Ctx,
    stack: *mut u64,
    target: *mut u8,
}

#[cfg(target_arch = "aarch64")]
lazy_static! {
    /// Switches stack and executes the provided callback.
    static ref SWITCH_STACK: unsafe extern "C" fn (stack: *mut u64, cb: extern "C" fn (*mut u8) -> u64, userdata: *mut u8) -> u64 = {
        let mut assembler = Assembler::new().unwrap();
        let offset = assembler.offset();
        dynasm!(
            assembler
            ; .arch aarch64
            ; sub x0, x0, 16
            ; mov x8, sp
            ; str x8, [x0, 0]
            ; str x30, [x0, 8]
            ; adr x30, >done
            ; mov sp, x0
            ; mov x0, x2
            ; br x1
            ; done:
            ; ldr x30, [sp, 8]
            ; ldr x8, [sp, 0]
            ; mov sp, x8
            ; br x30
        );
        let buf = assembler.finalize().unwrap();
        let ret = unsafe { mem::transmute(buf.ptr(offset)) };
        mem::forget(buf);
        ret
    };
}

pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
    signatures: Option<Arc<Map<SigIndex, FuncSig>>>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    function_labels: Option<HashMap<usize, (DynamicLabel, Option<AssemblyOffset>)>>,
    assembler: Option<Assembler>,
    func_import_count: usize,

    config: Option<Arc<CodegenConfig>>,
}

pub struct X64FunctionCode {
    local_function_id: usize,

    signatures: Arc<Map<SigIndex, FuncSig>>,
    function_signatures: Arc<Map<FuncIndex, SigIndex>>,
    fsm: FunctionStateMap,
    offset: usize,

    assembler: Option<Assembler>,
    function_labels: Option<HashMap<usize, (DynamicLabel, Option<AssemblyOffset>)>>,
    breakpoints: Option<
        HashMap<
            AssemblyOffset,
            Box<dyn Fn(BreakpointInfo) -> Result<(), Box<dyn Any + Send>> + Send + Sync + 'static>,
        >,
    >,
    returns: SmallVec<[WpType; 1]>,
    locals: Vec<Location>,
    num_params: usize,
    num_locals: usize,
    value_stack: Vec<Location>,
    control_stack: Vec<ControlFrame>,
    machine: Machine,
    unreachable_depth: usize,

    config: Arc<CodegenConfig>,
}

enum FuncPtrInner {}
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
struct FuncPtr(*const FuncPtrInner);
unsafe impl Send for FuncPtr {}
unsafe impl Sync for FuncPtr {}

pub struct X64ExecutionContext {
    #[allow(dead_code)]
    code: CodeMemory,
    function_pointers: Vec<FuncPtr>,
    function_offsets: Vec<AssemblyOffset>,
    signatures: Arc<Map<SigIndex, FuncSig>>,
    breakpoints: BreakpointMap,
    func_import_count: usize,
    msm: ModuleStateMap,
}

/// On-disk cache format.
/// Offsets are relative to the start of the executable image.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheImage {
    /// The executable image.
    code: Vec<u8>,

    /// Offsets to the start of each function. Including trampoline, if any.
    /// Trampolines are only present on AArch64.
    /// On x86-64, `function_pointers` are identical to `function_offsets`.
    function_pointers: Vec<usize>,

    /// Offsets to the start of each function after trampoline.
    function_offsets: Vec<usize>,

    /// Number of imported functions.
    func_import_count: usize,

    /// Module state map.
    msm: ModuleStateMap,
}

#[derive(Debug)]
pub struct ControlFrame {
    pub label: DynamicLabel,
    pub loop_like: bool,
    pub if_else: IfElseState,
    pub returns: SmallVec<[WpType; 1]>,
    pub value_stack_depth: usize,
    pub state: MachineState,
    pub state_diff_id: usize,
}

#[derive(Debug, Copy, Clone)]
pub enum IfElseState {
    None,
    If(DynamicLabel),
    Else,
}

pub struct SinglepassCache {
    buffer: Arc<[u8]>,
}

impl CacheGen for SinglepassCache {
    fn generate_cache(&self) -> Result<(Box<[u8]>, Memory), CacheError> {
        let mut memory = Memory::with_size_protect(self.buffer.len(), Protect::ReadWrite)
            .map_err(CacheError::SerializeError)?;

        let buffer = &*self.buffer;

        unsafe {
            memory.as_slice_mut()[..buffer.len()].copy_from_slice(buffer);
        }

        Ok(([].as_ref().into(), memory))
    }
}

impl RunnableModule for X64ExecutionContext {
    fn get_func(
        &self,
        _: &ModuleInfo,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        self.function_pointers[self.func_import_count..]
            .get(local_func_index.index())
            .and_then(|ptr| NonNull::new(ptr.0 as *mut vm::Func))
    }

    fn get_module_state_map(&self) -> Option<ModuleStateMap> {
        Some(self.msm.clone())
    }

    fn get_breakpoints(&self) -> Option<BreakpointMap> {
        Some(self.breakpoints.clone())
    }

    unsafe fn patch_local_function(&self, idx: usize, target_address: usize) -> bool {
        /*
        0:       48 b8 42 42 42 42 42 42 42 42   movabsq $4774451407313060418, %rax
        a:       49 bb 43 43 43 43 43 43 43 43   movabsq $4846791580151137091, %r11
        14:      41 ff e3        jmpq    *%r11
        */
        #[repr(packed)]
        struct LocalTrampoline {
            movabsq_rax: [u8; 2],
            addr_rax: u64,
            movabsq_r11: [u8; 2],
            addr_r11: u64,
            jmpq_r11: [u8; 3],
        }

        self.code.make_writable();

        let trampoline = &mut *(self.function_pointers[self.func_import_count + idx].0
            as *const LocalTrampoline as *mut LocalTrampoline);
        trampoline.movabsq_rax[0] = 0x48;
        trampoline.movabsq_rax[1] = 0xb8;
        trampoline.addr_rax = target_address as u64;
        trampoline.movabsq_r11[0] = 0x49;
        trampoline.movabsq_r11[1] = 0xbb;
        trampoline.addr_r11 =
            register_preservation_trampoline as unsafe extern "C" fn() as usize as u64;
        trampoline.jmpq_r11[0] = 0x41;
        trampoline.jmpq_r11[1] = 0xff;
        trampoline.jmpq_r11[2] = 0xe3;

        self.code.make_executable();
        true
    }

    fn get_trampoline(&self, _: &ModuleInfo, sig_index: SigIndex) -> Option<Wasm> {
        unsafe extern "C" fn invoke(
            _trampoline: Trampoline,
            ctx: *mut vm::Ctx,
            func: NonNull<vm::Func>,
            args: *const u64,
            rets: *mut u64,
            trap_info: *mut WasmTrapInfo,
            user_error: *mut Option<Box<dyn Any + Send>>,
            num_params_plus_one: Option<NonNull<c_void>>,
        ) -> bool {
            let rm: &Box<dyn RunnableModule> = &(&*(*ctx).module).runnable_module;

            let args =
                slice::from_raw_parts(args, num_params_plus_one.unwrap().as_ptr() as usize - 1);

            let ret = match protect_unix::call_protected(
                || {
                    #[cfg(target_arch = "x86_64")]
                    {
                        let args_reverse: SmallVec<[u64; 8]> = args.iter().cloned().rev().collect();
                        CONSTRUCT_STACK_AND_CALL_WASM(
                            args_reverse.as_ptr(),
                            args_reverse.as_ptr().offset(args_reverse.len() as isize),
                            ctx,
                            func.as_ptr(),
                        )
                    }
                    #[cfg(target_arch = "aarch64")]
                    {
                        struct CallCtx<'a> {
                            args: &'a [u64],
                            ctx: *mut vm::Ctx,
                            callable: NonNull<vm::Func>,
                        }
                        extern "C" fn call_fn(f: *mut u8) -> u64 {
                            unsafe {
                                let f = &*(f as *const CallCtx);
                                let callable: extern "C" fn(
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                    u64,
                                )
                                    -> u64 = std::mem::transmute(f.callable);
                                let mut args = f.args.iter();
                                callable(
                                    f.ctx as u64,
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                    args.next().cloned().unwrap_or(0),
                                )
                            }
                        }
                        let mut cctx = CallCtx {
                            args: &args,
                            ctx: ctx,
                            callable: func,
                        };
                        use libc::{
                            mmap, munmap, MAP_ANON, MAP_NORESERVE, MAP_PRIVATE, PROT_READ,
                            PROT_WRITE,
                        };
                        const STACK_SIZE: usize = 1048576 * 1024; // 1GB of virtual address space for stack.
                        let stack_ptr = mmap(
                            ::std::ptr::null_mut(),
                            STACK_SIZE,
                            PROT_READ | PROT_WRITE,
                            MAP_PRIVATE | MAP_ANON | MAP_NORESERVE,
                            -1,
                            0,
                        );
                        if stack_ptr as isize == -1 {
                            panic!("unable to allocate stack");
                        }
                        // TODO: Mark specific regions in the stack as PROT_NONE.
                        let ret = SWITCH_STACK(
                            (stack_ptr as *mut u8).offset(STACK_SIZE as isize) as *mut u64,
                            call_fn,
                            &mut cctx as *mut CallCtx as *mut u8,
                        );
                        munmap(stack_ptr, STACK_SIZE);
                        ret
                    }
                },
                rm.get_breakpoints(),
            ) {
                Ok(x) => {
                    if !rets.is_null() {
                        *rets = x;
                    }
                    true
                }
                Err(err) => {
                    match err {
                        protect_unix::CallProtError::Trap(info) => *trap_info = info,
                        protect_unix::CallProtError::Error(data) => *user_error = Some(data),
                    }
                    false
                }
            };
            ret
        }

        unsafe extern "C" fn dummy_trampoline(
            _: *mut vm::Ctx,
            _: NonNull<vm::Func>,
            _: *const u64,
            _: *mut u64,
        ) {
            unreachable!()
        }

        Some(unsafe {
            Wasm::from_raw_parts(
                dummy_trampoline,
                invoke,
                NonNull::new((self.signatures.get(sig_index).unwrap().params().len() + 1) as _), // +1 to keep it non-zero
            )
        })
    }

    unsafe fn do_early_trap(&self, data: Box<dyn Any + Send>) -> ! {
        protect_unix::TRAP_EARLY_DATA.with(|x| x.set(Some(data)));
        protect_unix::trigger_trap();
    }

    fn get_code(&self) -> Option<&[u8]> {
        Some(&self.code)
    }

    fn get_offsets(&self) -> Option<Vec<usize>> {
        Some(self.function_offsets.iter().map(|x| x.0).collect())
    }

    fn get_local_function_offsets(&self) -> Option<Vec<usize>> {
        Some(
            self.function_offsets[self.func_import_count..]
                .iter()
                .map(|x| x.0)
                .collect(),
        )
    }

    /// Returns the inline breakpoint size corresponding to an Architecture.
    fn get_inline_breakpoint_size(&self, arch: Architecture) -> Option<usize> {
        match arch {
            Architecture::X64 => Some(INLINE_BREAKPOINT_SIZE_X86_SINGLEPASS),
            Architecture::Aarch64 => Some(INLINE_BREAKPOINT_SIZE_AARCH64_SINGLEPASS),
        }
    }

    /// Attempts to read an inline breakpoint from the code.
    ///
    /// Inline breakpoints are detected by special instruction sequences that never
    /// appear in valid code.
    fn read_inline_breakpoint(&self, arch: Architecture, code: &[u8]) -> Option<InlineBreakpoint> {
        match arch {
            Architecture::X64 => {
                if code.len() < INLINE_BREAKPOINT_SIZE_X86_SINGLEPASS {
                    None
                } else if &code[..INLINE_BREAKPOINT_SIZE_X86_SINGLEPASS - 1]
                    == &[
                        0x0f, 0x0b, // ud2
                        0x0f, 0xb9, // ud
                        0xcd, 0xff, // int 0xff
                    ]
                {
                    Some(InlineBreakpoint {
                        size: INLINE_BREAKPOINT_SIZE_X86_SINGLEPASS,
                        ty: match code[INLINE_BREAKPOINT_SIZE_X86_SINGLEPASS - 1] {
                            0 => InlineBreakpointType::Middleware,
                            _ => return None,
                        },
                    })
                } else {
                    None
                }
            }
            Architecture::Aarch64 => {
                if code.len() < INLINE_BREAKPOINT_SIZE_AARCH64_SINGLEPASS {
                    None
                } else if &code[..INLINE_BREAKPOINT_SIZE_AARCH64_SINGLEPASS - 4]
                    == &[
                        0, 0, 0, 0, // udf #0
                        0xff, 0xff, 0x00, 0x00, // udf #65535
                    ]
                {
                    Some(InlineBreakpoint {
                        size: INLINE_BREAKPOINT_SIZE_AARCH64_SINGLEPASS,
                        ty: match code[INLINE_BREAKPOINT_SIZE_AARCH64_SINGLEPASS - 4] {
                            0 => InlineBreakpointType::Middleware,
                            _ => return None,
                        },
                    })
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

#[derive(Copy, Clone, Debug)]
struct CodegenConfig {
    memory_bound_check_mode: MemoryBoundCheckMode,
    enforce_stack_check: bool,
    track_state: bool,
}

impl ModuleCodeGenerator<X64FunctionCode, X64ExecutionContext, CodegenError>
    for X64ModuleCodeGenerator
{
    fn new() -> X64ModuleCodeGenerator {
        let a = Assembler::new().unwrap();

        X64ModuleCodeGenerator {
            functions: vec![],
            signatures: None,
            function_signatures: None,
            function_labels: Some(HashMap::new()),
            assembler: Some(a),
            func_import_count: 0,
            config: None,
        }
    }

    fn new_with_target(_: Option<String>, _: Option<String>, _: Option<String>) -> Self {
        unimplemented!("cross compilation is not available for singlepass backend")
    }

    fn backend_id() -> Backend {
        Backend::Singlepass
    }

    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn next_function(
        &mut self,
        _module_info: Arc<RwLock<ModuleInfo>>,
    ) -> Result<&mut X64FunctionCode, CodegenError> {
        let (mut assembler, mut function_labels, breakpoints) = match self.functions.last_mut() {
            Some(x) => (
                x.assembler.take().unwrap(),
                x.function_labels.take().unwrap(),
                x.breakpoints.take().unwrap(),
            ),
            None => (
                self.assembler.take().unwrap(),
                self.function_labels.take().unwrap(),
                HashMap::new(),
            ),
        };

        let begin_offset = assembler.offset();
        let begin_label_info = function_labels
            .entry(self.functions.len() + self.func_import_count)
            .or_insert_with(|| (assembler.new_dynamic_label(), None));

        begin_label_info.1 = Some(begin_offset);
        assembler.arch_emit_entry_trampoline();
        let begin_label = begin_label_info.0;
        let mut machine = Machine::new();
        machine.track_state = self.config.as_ref().unwrap().track_state;

        assembler.emit_label(begin_label);
        let code = X64FunctionCode {
            local_function_id: self.functions.len(),

            signatures: self.signatures.as_ref().unwrap().clone(),
            function_signatures: self.function_signatures.as_ref().unwrap().clone(),
            fsm: FunctionStateMap::new(new_machine_state(), self.functions.len(), 32, vec![]), // only a placeholder; this is initialized later in `begin_body`
            offset: begin_offset.0,

            assembler: Some(assembler),
            function_labels: Some(function_labels),
            breakpoints: Some(breakpoints),
            returns: smallvec![],
            locals: vec![],
            num_params: 0,
            num_locals: 0,
            value_stack: vec![],
            control_stack: vec![],
            machine,
            unreachable_depth: 0,
            config: self.config.as_ref().unwrap().clone(),
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(
        mut self,
        _: &ModuleInfo,
    ) -> Result<(X64ExecutionContext, Box<dyn CacheGen>), CodegenError> {
        let (assembler, function_labels, breakpoints) = match self.functions.last_mut() {
            Some(x) => (
                x.assembler.take().unwrap(),
                x.function_labels.take().unwrap(),
                x.breakpoints.take().unwrap(),
            ),
            None => (
                self.assembler.take().unwrap(),
                self.function_labels.take().unwrap(),
                HashMap::new(),
            ),
        };

        let total_size = assembler.get_offset().0;
        let _output = assembler.finalize().unwrap();
        let mut output = CodeMemory::new(_output.len());
        output[0.._output.len()].copy_from_slice(&_output);
        output.make_executable();

        let mut out_labels: Vec<FuncPtr> = vec![];
        let mut out_offsets: Vec<AssemblyOffset> = vec![];

        for i in 0..function_labels.len() {
            let (_, offset) = match function_labels.get(&i) {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: format!("label not found"),
                    });
                }
            };
            let offset = match offset {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: format!("offset is none"),
                    });
                }
            };
            out_labels.push(FuncPtr(
                unsafe { output.as_ptr().offset(offset.0 as isize) } as _,
            ));
            out_offsets.push(*offset);
        }

        let breakpoints: Arc<HashMap<_, _>> = Arc::new(
            breakpoints
                .into_iter()
                .map(|(offset, f)| {
                    (
                        unsafe { output.as_ptr().offset(offset.0 as isize) } as usize,
                        f,
                    )
                })
                .collect(),
        );

        let local_function_maps: BTreeMap<usize, FunctionStateMap> = self
            .functions
            .iter()
            .map(|x| (x.offset, x.fsm.clone()))
            .collect();

        let msm = ModuleStateMap {
            local_functions: local_function_maps,
            total_size,
        };

        let cache_image = CacheImage {
            code: output.to_vec(),
            function_pointers: out_labels
                .iter()
                .map(|x| {
                    (x.0 as usize)
                        .checked_sub(output.as_ptr() as usize)
                        .unwrap()
                })
                .collect(),
            function_offsets: out_offsets.iter().map(|x| x.0 as usize).collect(),
            func_import_count: self.func_import_count,
            msm: msm.clone(),
        };

        let cache = SinglepassCache {
            buffer: Arc::from(bincode::serialize(&cache_image).unwrap().into_boxed_slice()),
        };

        Ok((
            X64ExecutionContext {
                code: output,
                signatures: self.signatures.as_ref().unwrap().clone(),
                breakpoints: breakpoints,
                func_import_count: self.func_import_count,
                function_pointers: out_labels,
                function_offsets: out_offsets,
                msm: msm,
            },
            Box::new(cache),
        ))
    }

    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError> {
        self.signatures = Some(Arc::new(signatures));
        Ok(())
    }

    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError> {
        self.function_signatures = Some(Arc::new(assoc));
        Ok(())
    }

    fn feed_import_function(&mut self) -> Result<(), CodegenError> {
        let labels = self.function_labels.as_mut().unwrap();
        let id = labels.len();

        let a = self.assembler.as_mut().unwrap();
        let offset = a.offset();
        a.arch_emit_entry_trampoline();
        let label = a.get_label();
        a.emit_label(label);
        labels.insert(id, (label, Some(offset)));

        // Emits a tail call trampoline that loads the address of the target import function
        // from Ctx and jumps to it.

        let imported_funcs_addr = vm::Ctx::offset_imported_funcs();
        let imported_func = vm::ImportedFunc::size() as usize * id;
        let imported_func_addr = imported_func + vm::ImportedFunc::offset_func() as usize;
        let imported_func_ctx_addr = imported_func + vm::ImportedFunc::offset_func_ctx() as usize;
        let imported_func_ctx_vmctx_addr = vm::FuncCtx::offset_vmctx() as usize;

        a.emit_mov(
            Size::S64,
            Location::Memory(GPR::RDI, imported_funcs_addr as i32),
            Location::GPR(GPR::RAX),
        );
        a.emit_mov(
            Size::S64,
            Location::Memory(GPR::RAX, imported_func_ctx_addr as i32),
            Location::GPR(GPR::RDI),
        );
        a.emit_mov(
            Size::S64,
            Location::Memory(GPR::RDI, imported_func_ctx_vmctx_addr as i32),
            Location::GPR(GPR::RDI),
        );
        a.emit_mov(
            Size::S64,
            Location::Memory(GPR::RAX, imported_func_addr as i32),
            Location::GPR(GPR::RAX),
        );
        a.emit_host_redirection(GPR::RAX);

        self.func_import_count += 1;

        Ok(())
    }

    fn feed_compiler_config(&mut self, config: &CompilerConfig) -> Result<(), CodegenError> {
        self.config = Some(Arc::new(CodegenConfig {
            memory_bound_check_mode: config.memory_bound_check_mode,
            enforce_stack_check: config.enforce_stack_check,
            track_state: config.track_state,
        }));
        Ok(())
    }
    unsafe fn from_cache(artifact: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        let (info, _, memory) = artifact.consume();

        let cache_image: CacheImage = bincode::deserialize(memory.as_slice())
            .map_err(|x| CacheError::DeserializeError(format!("{:?}", x)))?;

        let mut code_mem = CodeMemory::new(cache_image.code.len());
        code_mem[0..cache_image.code.len()].copy_from_slice(&cache_image.code);
        code_mem.make_executable();

        let function_pointers: Vec<FuncPtr> = cache_image
            .function_pointers
            .iter()
            .map(|&x| FuncPtr(code_mem.as_ptr().offset(x as isize) as *const FuncPtrInner))
            .collect();
        let function_offsets: Vec<AssemblyOffset> = cache_image
            .function_offsets
            .iter()
            .cloned()
            .map(AssemblyOffset)
            .collect();

        let ec = X64ExecutionContext {
            code: code_mem,
            function_pointers,
            function_offsets,
            signatures: Arc::new(info.signatures.clone()),
            breakpoints: Arc::new(HashMap::new()),
            func_import_count: cache_image.func_import_count,
            msm: cache_image.msm,
        };
        Ok(ModuleInner {
            runnable_module: Arc::new(Box::new(ec)),
            cache_gen: Box::new(SinglepassCache {
                buffer: Arc::from(memory.as_slice().to_vec().into_boxed_slice()),
            }),
            info,
        })
    }
}

impl X64FunctionCode {
    fn mark_trappable(
        a: &mut Assembler,
        m: &Machine,
        fsm: &mut FunctionStateMap,
        control_stack: &mut [ControlFrame],
    ) {
        let state_diff_id = Self::get_state_diff(m, fsm, control_stack);
        let offset = a.get_offset().0;
        fsm.trappable_offsets.insert(
            offset,
            OffsetInfo {
                end_offset: offset + 1,
                activate_offset: offset,
                diff_id: state_diff_id,
            },
        );
        fsm.wasm_offset_to_target_offset
            .insert(m.state.wasm_inst_offset, SuspendOffset::Trappable(offset));
    }

    /// Moves `loc` to a valid location for `div`/`idiv`.
    fn emit_relaxed_xdiv(
        a: &mut Assembler,
        m: &mut Machine,
        op: fn(&mut Assembler, Size, Location),
        sz: Size,
        loc: Location,
        fsm: &mut FunctionStateMap,
        control_stack: &mut [ControlFrame],
    ) {
        m.state.wasm_stack_private_depth += 1;
        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                a.emit_mov(sz, loc, Location::GPR(GPR::RCX)); // must not be used during div (rax, rdx)
                Self::mark_trappable(a, m, fsm, control_stack);
                op(a, sz, Location::GPR(GPR::RCX));
            }
            _ => {
                Self::mark_trappable(a, m, fsm, control_stack);
                op(a, sz, loc);
            }
        }
        m.state.wasm_stack_private_depth -= 1;
    }

    /// Moves `src` and `dst` to valid locations for `movzx`/`movsx`.
    fn emit_relaxed_zx_sx(
        a: &mut Assembler,
        m: &mut Machine,
        op: fn(&mut Assembler, Size, Location, Size, Location),
        sz_src: Size,
        mut src: Location,
        sz_dst: Size,
        dst: Location,
    ) -> Result<(), CodegenError> {
        let inner = |m: &mut Machine, a: &mut Assembler, src: Location| match dst {
            Location::Imm32(_) | Location::Imm64(_) => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_zx_sx dst Imm: unreachable code"),
                })
            }
            Location::Memory(_, _) => {
                let tmp_dst = m.acquire_temp_gpr().unwrap();
                op(a, sz_src, src, sz_dst, Location::GPR(tmp_dst));
                a.emit_mov(Size::S64, Location::GPR(tmp_dst), dst);

                m.release_temp_gpr(tmp_dst);
                Ok(())
            }
            Location::GPR(_) => {
                op(a, sz_src, src, sz_dst, dst);
                Ok(())
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_zx_sx dst: unreachable code"),
                })
            }
        };

        match src {
            Location::Imm32(_) | Location::Imm64(_) => {
                let tmp_src = m.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S64, src, Location::GPR(tmp_src));
                src = Location::GPR(tmp_src);

                inner(m, a, src)?;

                m.release_temp_gpr(tmp_src);
            }
            Location::GPR(_) | Location::Memory(_, _) => inner(m, a, src)?,
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_zx_sx src: unreachable code"),
                })
            }
        }
        Ok(())
    }

    /// Moves `src` and `dst` to valid locations for generic instructions.
    fn emit_relaxed_binop(
        a: &mut Assembler,
        m: &mut Machine,
        op: fn(&mut Assembler, Size, Location, Location),
        sz: Size,
        src: Location,
        dst: Location,
    ) {
        enum RelaxMode {
            Direct,
            SrcToGPR,
            DstToGPR,
            BothToGPR,
        }
        let mode = match (src, dst) {
            (Location::GPR(_), Location::GPR(_))
                if (op as *const u8 == Assembler::emit_imul as *const u8) =>
            {
                RelaxMode::Direct
            }
            _ if (op as *const u8 == Assembler::emit_imul as *const u8) => RelaxMode::BothToGPR,

            (Location::Memory(_, _), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (Location::Imm64(_), Location::Imm64(_)) | (Location::Imm64(_), Location::Imm32(_)) => {
                RelaxMode::BothToGPR
            }
            (_, Location::Imm32(_)) | (_, Location::Imm64(_)) => RelaxMode::DstToGPR,
            (Location::Imm64(_), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (Location::Imm64(_), Location::GPR(_))
                if (op as *const u8 != Assembler::emit_mov as *const u8) =>
            {
                RelaxMode::SrcToGPR
            }
            (_, Location::XMM(_)) => RelaxMode::SrcToGPR,
            _ => RelaxMode::Direct,
        };

        match mode {
            RelaxMode::SrcToGPR => {
                let temp = m.acquire_temp_gpr().unwrap();
                a.emit_mov(sz, src, Location::GPR(temp));
                op(a, sz, Location::GPR(temp), dst);
                m.release_temp_gpr(temp);
            }
            RelaxMode::DstToGPR => {
                let temp = m.acquire_temp_gpr().unwrap();
                a.emit_mov(sz, dst, Location::GPR(temp));
                op(a, sz, src, Location::GPR(temp));
                m.release_temp_gpr(temp);
            }
            RelaxMode::BothToGPR => {
                let temp_src = m.acquire_temp_gpr().unwrap();
                let temp_dst = m.acquire_temp_gpr().unwrap();
                a.emit_mov(sz, src, Location::GPR(temp_src));
                a.emit_mov(sz, dst, Location::GPR(temp_dst));
                op(a, sz, Location::GPR(temp_src), Location::GPR(temp_dst));
                match dst {
                    Location::Memory(_, _) | Location::GPR(_) => {
                        a.emit_mov(sz, Location::GPR(temp_dst), dst);
                    }
                    _ => {}
                }
                m.release_temp_gpr(temp_dst);
                m.release_temp_gpr(temp_src);
            }
            RelaxMode::Direct => {
                op(a, sz, src, dst);
            }
        }
    }

    /// Moves `src1` and `src2` to valid locations and possibly adds a layer of indirection for `dst` for AVX instructions.
    fn emit_relaxed_avx(
        a: &mut Assembler,
        m: &mut Machine,
        op: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CodegenError> {
        Self::emit_relaxed_avx_base(
            a,
            m,
            |a, _, src1, src2, dst| op(a, src1, src2, dst),
            src1,
            src2,
            dst,
        )?;
        Ok(())
    }

    /// Moves `src1` and `src2` to valid locations and possibly adds a layer of indirection for `dst` for AVX instructions.
    fn emit_relaxed_avx_base<F: FnOnce(&mut Assembler, &mut Machine, XMM, XMMOrMemory, XMM)>(
        a: &mut Assembler,
        m: &mut Machine,
        op: F,
        src1: Location,
        src2: Location,
        dst: Location,
    ) -> Result<(), CodegenError> {
        let tmp1 = m.acquire_temp_xmm().unwrap();
        let tmp2 = m.acquire_temp_xmm().unwrap();
        let tmp3 = m.acquire_temp_xmm().unwrap();
        let tmpg = m.acquire_temp_gpr().unwrap();

        let src1 = match src1 {
            Location::XMM(x) => x,
            Location::GPR(_) | Location::Memory(_, _) => {
                a.emit_mov(Size::S64, src1, Location::XMM(tmp1));
                tmp1
            }
            Location::Imm32(_) => {
                a.emit_mov(Size::S32, src1, Location::GPR(tmpg));
                a.emit_mov(Size::S32, Location::GPR(tmpg), Location::XMM(tmp1));
                tmp1
            }
            Location::Imm64(_) => {
                a.emit_mov(Size::S64, src1, Location::GPR(tmpg));
                a.emit_mov(Size::S64, Location::GPR(tmpg), Location::XMM(tmp1));
                tmp1
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_avx_base src1: unreachable code"),
                })
            }
        };

        let src2 = match src2 {
            Location::XMM(x) => XMMOrMemory::XMM(x),
            Location::Memory(base, disp) => XMMOrMemory::Memory(base, disp),
            Location::GPR(_) => {
                a.emit_mov(Size::S64, src2, Location::XMM(tmp2));
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm32(_) => {
                a.emit_mov(Size::S32, src2, Location::GPR(tmpg));
                a.emit_mov(Size::S32, Location::GPR(tmpg), Location::XMM(tmp2));
                XMMOrMemory::XMM(tmp2)
            }
            Location::Imm64(_) => {
                a.emit_mov(Size::S64, src2, Location::GPR(tmpg));
                a.emit_mov(Size::S64, Location::GPR(tmpg), Location::XMM(tmp2));
                XMMOrMemory::XMM(tmp2)
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_avx_base src2: unreachable code"),
                })
            }
        };

        match dst {
            Location::XMM(x) => {
                op(a, m, src1, src2, x);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                op(a, m, src1, src2, tmp3);
                a.emit_mov(Size::S64, Location::XMM(tmp3), dst);
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_relaxed_avx_base dst: unreachable code"),
                })
            }
        }

        m.release_temp_gpr(tmpg);
        m.release_temp_xmm(tmp3);
        m.release_temp_xmm(tmp2);
        m.release_temp_xmm(tmp1);
        Ok(())
    }

    /// I32 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        // Using Red Zone here.
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::I32, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];

        if loc_a != ret {
            let tmp = m.acquire_temp_gpr().unwrap();
            Self::emit_relaxed_binop(
                a,
                m,
                Assembler::emit_mov,
                Size::S32,
                loc_a,
                Location::GPR(tmp),
            );
            Self::emit_relaxed_binop(a, m, f, Size::S32, loc_b, Location::GPR(tmp));
            Self::emit_relaxed_binop(
                a,
                m,
                Assembler::emit_mov,
                Size::S32,
                Location::GPR(tmp),
                ret,
            );
            m.release_temp_gpr(tmp);
        } else {
            Self::emit_relaxed_binop(a, m, f, Size::S32, loc_b, ret);
        }

        value_stack.push(ret);
    }

    /// I64 binary operation with both operands popped from the virtual stack.
    fn emit_binop_i64(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        // Using Red Zone here.
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::I64, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];

        if loc_a != ret {
            let tmp = m.acquire_temp_gpr().unwrap();
            Self::emit_relaxed_binop(
                a,
                m,
                Assembler::emit_mov,
                Size::S64,
                loc_a,
                Location::GPR(tmp),
            );
            Self::emit_relaxed_binop(a, m, f, Size::S64, loc_b, Location::GPR(tmp));
            Self::emit_relaxed_binop(
                a,
                m,
                Assembler::emit_mov,
                Size::S64,
                Location::GPR(tmp),
                ret,
            );
            m.release_temp_gpr(tmp);
        } else {
            Self::emit_relaxed_binop(a, m, f, Size::S64, loc_b, ret);
        }

        value_stack.push(ret);
    }

    /// I32 comparison with `loc_b` from input.
    fn emit_cmpop_i32_dynamic_b(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        c: Condition,
        loc_b: Location,
    ) -> Result<(), CodegenError> {
        // Using Red Zone here.
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());

        let ret = m.acquire_locations(
            a,
            &[(WpType::I32, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];
        match ret {
            Location::GPR(x) => {
                Self::emit_relaxed_binop(a, m, Assembler::emit_cmp, Size::S32, loc_b, loc_a);
                a.emit_set(c, x);
                a.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(x));
            }
            Location::Memory(_, _) => {
                let tmp = m.acquire_temp_gpr().unwrap();
                Self::emit_relaxed_binop(a, m, Assembler::emit_cmp, Size::S32, loc_b, loc_a);
                a.emit_set(c, tmp);
                a.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(tmp));
                a.emit_mov(Size::S32, Location::GPR(tmp), ret);
                m.release_temp_gpr(tmp);
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_cmpop_i32_dynamic_b ret: unreachable code"),
                })
            }
        }
        value_stack.push(ret);
        Ok(())
    }

    /// I32 comparison with both operands popped from the virtual stack.
    fn emit_cmpop_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        c: Condition,
    ) -> Result<(), CodegenError> {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        Self::emit_cmpop_i32_dynamic_b(a, m, value_stack, c, loc_b)?;
        Ok(())
    }

    /// I64 comparison with `loc_b` from input.
    fn emit_cmpop_i64_dynamic_b(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        c: Condition,
        loc_b: Location,
    ) -> Result<(), CodegenError> {
        // Using Red Zone here.
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());

        let ret = m.acquire_locations(
            a,
            &[(WpType::I32, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];
        match ret {
            Location::GPR(x) => {
                Self::emit_relaxed_binop(a, m, Assembler::emit_cmp, Size::S64, loc_b, loc_a);
                a.emit_set(c, x);
                a.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(x));
            }
            Location::Memory(_, _) => {
                let tmp = m.acquire_temp_gpr().unwrap();
                Self::emit_relaxed_binop(a, m, Assembler::emit_cmp, Size::S64, loc_b, loc_a);
                a.emit_set(c, tmp);
                a.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(tmp));
                a.emit_mov(Size::S32, Location::GPR(tmp), ret);
                m.release_temp_gpr(tmp);
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_cmpop_i64_dynamic_b ret: unreachable code"),
                })
            }
        }
        value_stack.push(ret);
        Ok(())
    }

    /// I64 comparison with both operands popped from the virtual stack.
    fn emit_cmpop_i64(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        c: Condition,
    ) -> Result<(), CodegenError> {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        Self::emit_cmpop_i64_dynamic_b(a, m, value_stack, c, loc_b)?;
        Ok(())
    }

    /// I32 `lzcnt`/`tzcnt`/`popcnt` with operand popped from the virtual stack.
    fn emit_xcnt_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) -> Result<(), CodegenError> {
        let loc = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::I32, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];

        match loc {
            Location::Imm32(_) => {
                let tmp = m.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(tmp));
                if let Location::Memory(_, _) = ret {
                    let out_tmp = m.acquire_temp_gpr().unwrap();
                    f(a, Size::S32, Location::GPR(tmp), Location::GPR(out_tmp));
                    a.emit_mov(Size::S32, Location::GPR(out_tmp), ret);
                    m.release_temp_gpr(out_tmp);
                } else {
                    f(a, Size::S32, Location::GPR(tmp), ret);
                }
                m.release_temp_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = m.acquire_temp_gpr().unwrap();
                    f(a, Size::S32, loc, Location::GPR(out_tmp));
                    a.emit_mov(Size::S32, Location::GPR(out_tmp), ret);
                    m.release_temp_gpr(out_tmp);
                } else {
                    f(a, Size::S32, loc, ret);
                }
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_xcnt_i32 loc: unreachable code"),
                })
            }
        }
        value_stack.push(ret);
        Ok(())
    }

    /// I64 `lzcnt`/`tzcnt`/`popcnt` with operand popped from the virtual stack.
    fn emit_xcnt_i64(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) -> Result<(), CodegenError> {
        let loc = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::I64, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];

        match loc {
            Location::Imm64(_) | Location::Imm32(_) => {
                let tmp = m.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S64, loc, Location::GPR(tmp));
                if let Location::Memory(_, _) = ret {
                    let out_tmp = m.acquire_temp_gpr().unwrap();
                    f(a, Size::S64, Location::GPR(tmp), Location::GPR(out_tmp));
                    a.emit_mov(Size::S64, Location::GPR(out_tmp), ret);
                    m.release_temp_gpr(out_tmp);
                } else {
                    f(a, Size::S64, Location::GPR(tmp), ret);
                }
                m.release_temp_gpr(tmp);
            }
            Location::Memory(_, _) | Location::GPR(_) => {
                if let Location::Memory(_, _) = ret {
                    let out_tmp = m.acquire_temp_gpr().unwrap();
                    f(a, Size::S64, loc, Location::GPR(out_tmp));
                    a.emit_mov(Size::S64, Location::GPR(out_tmp), ret);
                    m.release_temp_gpr(out_tmp);
                } else {
                    f(a, Size::S64, loc, ret);
                }
            }
            _ => {
                return Err(CodegenError {
                    message: format!("emit_xcnt_i64 loc: unreachable code"),
                })
            }
        }
        value_stack.push(ret);
        Ok(())
    }

    /// I32 shift with both operands popped from the virtual stack.
    fn emit_shift_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::I32, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];

        a.emit_mov(Size::S32, loc_b, Location::GPR(GPR::RCX));

        if loc_a != ret {
            Self::emit_relaxed_binop(a, m, Assembler::emit_mov, Size::S32, loc_a, ret);
        }

        f(a, Size::S32, Location::GPR(GPR::RCX), ret);
        value_stack.push(ret);
    }

    /// I64 shift with both operands popped from the virtual stack.
    fn emit_shift_i64(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::I64, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];

        a.emit_mov(Size::S64, loc_b, Location::GPR(GPR::RCX));

        if loc_a != ret {
            Self::emit_relaxed_binop(a, m, Assembler::emit_mov, Size::S64, loc_a, ret);
        }

        f(a, Size::S64, Location::GPR(GPR::RCX), ret);
        value_stack.push(ret);
    }

    /// Floating point (AVX) binary operation with both operands popped from the virtual stack.
    fn emit_fp_binop_avx(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
    ) -> Result<(), CodegenError> {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::F64, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];
        value_stack.push(ret);

        Self::emit_relaxed_avx(a, m, f, loc_a, loc_b, ret)?;
        Ok(())
    }

    /// Floating point (AVX) comparison with both operands popped from the virtual stack.
    fn emit_fp_cmpop_avx(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
    ) -> Result<(), CodegenError> {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::I32, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];
        value_stack.push(ret);

        Self::emit_relaxed_avx(a, m, f, loc_a, loc_b, ret)?;

        // Workaround for behavior inconsistency among different backing implementations.
        // (all bits or only the least significant bit are set to one?)
        a.emit_and(Size::S32, Location::Imm32(1), ret);
        Ok(())
    }

    /// Floating point (AVX) binary operation with both operands popped from the virtual stack.
    fn emit_fp_unop_avx(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<Location>,
        f: fn(&mut Assembler, XMM, XMMOrMemory, XMM),
    ) -> Result<(), CodegenError> {
        let loc = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(
            a,
            &[(WpType::F64, MachineValue::WasmStack(value_stack.len()))],
            false,
        )[0];
        value_stack.push(ret);

        Self::emit_relaxed_avx(a, m, f, loc, loc, ret)?;
        Ok(())
    }

    /// Emits a System V call sequence.
    ///
    /// This function must not use RAX before `cb` is called.
    fn emit_call_sysv<I: Iterator<Item = Location>, F: FnOnce(&mut Assembler)>(
        a: &mut Assembler,
        m: &mut Machine,
        cb: F,
        params: I,
        state_context: Option<(&mut FunctionStateMap, &mut [ControlFrame])>,
    ) -> Result<(), CodegenError> {
        // Values pushed in this function are above the shadow region.
        m.state.stack_values.push(MachineValue::ExplicitShadow);

        let params: Vec<_> = params.collect();

        // Save used GPRs.
        let used_gprs = m.get_used_gprs();
        for r in used_gprs.iter() {
            a.emit_push(Size::S64, Location::GPR(*r));
            let content = m.state.register_values[X64Register::GPR(*r).to_index().0].clone();
            if content == MachineValue::Undefined {
                return Err(CodegenError {
                    message: format!("emit_call_sysv: Undefined used_gprs content"),
                });
            }
            m.state.stack_values.push(content);
        }

        // Save used XMM registers.
        let used_xmms = m.get_used_xmms();
        if used_xmms.len() > 0 {
            a.emit_sub(
                Size::S64,
                Location::Imm32((used_xmms.len() * 8) as u32),
                Location::GPR(GPR::RSP),
            );

            // FIXME: Possible dynasm bug. This is a workaround.
            // Using RSP as the source/destination operand of a `mov` instruction produces invalid code.
            a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RCX));
            for (i, r) in used_xmms.iter().enumerate() {
                a.emit_mov(
                    Size::S64,
                    Location::XMM(*r),
                    Location::Memory(GPR::RCX, (i * 8) as i32),
                );
            }
            for r in used_xmms.iter().rev() {
                let content = m.state.register_values[X64Register::XMM(*r).to_index().0].clone();
                if content == MachineValue::Undefined {
                    return Err(CodegenError {
                        message: format!("emit_call_sysv: Undefined used_xmms content"),
                    });
                }
                m.state.stack_values.push(content);
            }
        }

        let mut stack_offset: usize = 0;

        // Calculate stack offset.
        for (i, _param) in params.iter().enumerate() {
            let loc = Machine::get_param_location(1 + i);
            match loc {
                Location::Memory(_, _) => {
                    stack_offset += 8;
                }
                _ => {}
            }
        }

        // Align stack to 16 bytes.
        if (m.get_stack_offset() + used_gprs.len() * 8 + used_xmms.len() * 8 + stack_offset) % 16
            != 0
        {
            a.emit_sub(Size::S64, Location::Imm32(8), Location::GPR(GPR::RSP));
            stack_offset += 8;
            m.state.stack_values.push(MachineValue::Undefined);
        }

        let mut call_movs: Vec<(Location, GPR)> = vec![];

        // Prepare register & stack parameters.
        for (i, param) in params.iter().enumerate().rev() {
            let loc = Machine::get_param_location(1 + i);
            match loc {
                Location::GPR(x) => {
                    call_movs.push((*param, x));
                }
                Location::Memory(_, _) => {
                    match *param {
                        Location::GPR(x) => {
                            let content =
                                m.state.register_values[X64Register::GPR(x).to_index().0].clone();
                            // FIXME: There might be some corner cases (release -> emit_call_sysv -> acquire?) that cause this assertion to fail.
                            // Hopefully nothing would be incorrect at runtime.

                            //assert!(content != MachineValue::Undefined);
                            m.state.stack_values.push(content);
                        }
                        Location::XMM(x) => {
                            let content =
                                m.state.register_values[X64Register::XMM(x).to_index().0].clone();
                            //assert!(content != MachineValue::Undefined);
                            m.state.stack_values.push(content);
                        }
                        Location::Memory(reg, offset) => {
                            if reg != GPR::RBP {
                                return Err(CodegenError {
                                    message: format!("emit_call_sysv loc param: unreachable code"),
                                });
                            }
                            m.state
                                .stack_values
                                .push(MachineValue::CopyStackBPRelative(offset));
                            // TODO: Read value at this offset
                        }
                        _ => {
                            m.state.stack_values.push(MachineValue::Undefined);
                        }
                    }
                    match *param {
                        // Dynasm bug: RSP in memory operand does not work
                        Location::Imm64(_) | Location::XMM(_) => {
                            a.emit_mov(
                                Size::S64,
                                Location::GPR(GPR::RAX),
                                Location::XMM(XMM::XMM0),
                            );
                            a.emit_mov(
                                Size::S64,
                                Location::GPR(GPR::RCX),
                                Location::XMM(XMM::XMM1),
                            );
                            a.emit_sub(Size::S64, Location::Imm32(8), Location::GPR(GPR::RSP));
                            a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RCX));
                            a.emit_mov(Size::S64, *param, Location::GPR(GPR::RAX));
                            a.emit_mov(
                                Size::S64,
                                Location::GPR(GPR::RAX),
                                Location::Memory(GPR::RCX, 0),
                            );
                            a.emit_mov(
                                Size::S64,
                                Location::XMM(XMM::XMM0),
                                Location::GPR(GPR::RAX),
                            );
                            a.emit_mov(
                                Size::S64,
                                Location::XMM(XMM::XMM1),
                                Location::GPR(GPR::RCX),
                            );
                        }
                        _ => a.emit_push(Size::S64, *param),
                    }
                }
                _ => {
                    return Err(CodegenError {
                        message: format!("emit_call_sysv loc: unreachable code"),
                    })
                }
            }
        }

        // Sort register moves so that register are not overwritten before read.
        sort_call_movs(&mut call_movs);

        // Emit register moves.
        for (loc, gpr) in call_movs {
            if loc != Location::GPR(gpr) {
                a.emit_mov(Size::S64, loc, Location::GPR(gpr));
            }
        }

        // Put vmctx as the first parameter.
        a.emit_mov(
            Size::S64,
            Location::GPR(Machine::get_vmctx_reg()),
            Machine::get_param_location(0),
        ); // vmctx

        if (m.state.stack_values.len() % 2) != 1 {
            return Err(CodegenError {
                message: format!("emit_call_sysv: explicit shadow takes one slot"),
            });
        }

        cb(a);

        // Offset needs to be after the 'call' instruction.
        if let Some((fsm, control_stack)) = state_context {
            let state_diff_id = Self::get_state_diff(m, fsm, control_stack);
            let offset = a.get_offset().0;
            fsm.call_offsets.insert(
                offset,
                OffsetInfo {
                    end_offset: offset + 1,
                    activate_offset: offset,
                    diff_id: state_diff_id,
                },
            );
            fsm.wasm_offset_to_target_offset
                .insert(m.state.wasm_inst_offset, SuspendOffset::Call(offset));
        }

        // Restore stack.
        if stack_offset > 0 {
            a.emit_add(
                Size::S64,
                Location::Imm32(stack_offset as u32),
                Location::GPR(GPR::RSP),
            );
            if (stack_offset % 8) != 0 {
                return Err(CodegenError {
                    message: format!("emit_call_sysv: Bad restoring stack alignement"),
                });
            }
            for _ in 0..stack_offset / 8 {
                m.state.stack_values.pop().unwrap();
            }
        }

        // Restore XMMs.
        if used_xmms.len() > 0 {
            // FIXME: Possible dynasm bug. This is a workaround.
            a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RDX));
            for (i, r) in used_xmms.iter().enumerate() {
                a.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RDX, (i * 8) as i32),
                    Location::XMM(*r),
                );
            }
            a.emit_add(
                Size::S64,
                Location::Imm32((used_xmms.len() * 8) as u32),
                Location::GPR(GPR::RSP),
            );
            for _ in 0..used_xmms.len() {
                m.state.stack_values.pop().unwrap();
            }
        }

        // Restore GPRs.
        for r in used_gprs.iter().rev() {
            a.emit_pop(Size::S64, Location::GPR(*r));
            m.state.stack_values.pop().unwrap();
        }

        if m.state.stack_values.pop().unwrap() != MachineValue::ExplicitShadow {
            return Err(CodegenError {
                message: format!("emit_call_sysv: Popped value is not ExplicitShadow"),
            });
        }
        Ok(())
    }

    /// Emits a System V call sequence, specialized for labels as the call target.
    fn emit_call_sysv_label<I: Iterator<Item = Location>>(
        a: &mut Assembler,
        m: &mut Machine,
        label: DynamicLabel,
        params: I,
        state_context: Option<(&mut FunctionStateMap, &mut [ControlFrame])>,
    ) -> Result<(), CodegenError> {
        Self::emit_call_sysv(a, m, |a| a.emit_call_label(label), params, state_context)?;
        Ok(())
    }

    /// Emits a memory operation.
    fn emit_memory_op<F: FnOnce(&mut Assembler, &mut Machine, GPR) -> Result<(), CodegenError>>(
        module_info: &ModuleInfo,
        config: &CodegenConfig,
        a: &mut Assembler,
        m: &mut Machine,
        addr: Location,
        memarg: &MemoryImmediate,
        check_alignment: bool,
        value_size: usize,
        cb: F,
    ) -> Result<(), CodegenError> {
        // If the memory is dynamic, we need to do bound checking at runtime.
        let mem_desc = match MemoryIndex::new(0).local_or_import(module_info) {
            LocalOrImport::Local(local_mem_index) => &module_info.memories[local_mem_index],
            LocalOrImport::Import(import_mem_index) => {
                &module_info.imported_memories[import_mem_index].1
            }
        };
        let need_check = match config.memory_bound_check_mode {
            MemoryBoundCheckMode::Default => match mem_desc.memory_type() {
                MemoryType::Dynamic => true,
                MemoryType::Static | MemoryType::SharedStatic => false,
            },
            MemoryBoundCheckMode::Enable => true,
            MemoryBoundCheckMode::Disable => false,
        };

        let tmp_addr = m.acquire_temp_gpr().unwrap();
        let tmp_base = m.acquire_temp_gpr().unwrap();

        // Load base into temporary register.
        a.emit_mov(
            Size::S64,
            Location::Memory(
                Machine::get_vmctx_reg(),
                vm::Ctx::offset_memory_base() as i32,
            ),
            Location::GPR(tmp_base),
        );

        if need_check {
            let tmp_bound = m.acquire_temp_gpr().unwrap();

            a.emit_mov(
                Size::S64,
                Location::Memory(
                    Machine::get_vmctx_reg(),
                    vm::Ctx::offset_memory_bound() as i32,
                ),
                Location::GPR(tmp_bound),
            );
            // Adds base to bound so `tmp_bound` now holds the end of linear memory.
            a.emit_add(Size::S64, Location::GPR(tmp_base), Location::GPR(tmp_bound));
            a.emit_mov(Size::S32, addr, Location::GPR(tmp_addr));

            // This branch is used for emitting "faster" code for the special case of (offset + value_size) not exceeding u32 range.
            match (memarg.offset as u32).checked_add(value_size as u32) {
                Some(0) => {}
                Some(x) => {
                    a.emit_add(Size::S64, Location::Imm32(x), Location::GPR(tmp_addr));
                }
                None => {
                    a.emit_add(
                        Size::S64,
                        Location::Imm32(memarg.offset as u32),
                        Location::GPR(tmp_addr),
                    );
                    a.emit_add(
                        Size::S64,
                        Location::Imm32(value_size as u32),
                        Location::GPR(tmp_addr),
                    );
                }
            }

            // Trap if the end address of the requested area is above that of the linear memory.
            a.emit_add(Size::S64, Location::GPR(tmp_base), Location::GPR(tmp_addr));
            a.emit_cmp(Size::S64, Location::GPR(tmp_bound), Location::GPR(tmp_addr));
            a.emit_conditional_trap(Condition::Above);

            m.release_temp_gpr(tmp_bound);
        }

        // Calculates the real address, and loads from it.
        a.emit_mov(Size::S32, addr, Location::GPR(tmp_addr));
        if memarg.offset != 0 {
            a.emit_add(
                Size::S64,
                Location::Imm32(memarg.offset as u32),
                Location::GPR(tmp_addr),
            );
        }
        a.emit_add(Size::S64, Location::GPR(tmp_base), Location::GPR(tmp_addr));
        m.release_temp_gpr(tmp_base);

        let align = match memarg.flags & 3 {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            _ => {
                return Err(CodegenError {
                    message: format!("emit_memory_op align: unreachable value"),
                })
            }
        };
        if check_alignment && align != 1 {
            let tmp_aligncheck = m.acquire_temp_gpr().unwrap();
            a.emit_mov(
                Size::S32,
                Location::GPR(tmp_addr),
                Location::GPR(tmp_aligncheck),
            );
            a.emit_and(
                Size::S64,
                Location::Imm32(align - 1),
                Location::GPR(tmp_aligncheck),
            );
            a.emit_conditional_trap(Condition::NotEqual);
            m.release_temp_gpr(tmp_aligncheck);
        }

        cb(a, m, tmp_addr)?;

        m.release_temp_gpr(tmp_addr);
        Ok(())
    }

    /// Emits a memory operation.
    fn emit_compare_and_swap<F: FnOnce(&mut Assembler, &mut Machine, GPR, GPR)>(
        module_info: &ModuleInfo,
        config: &CodegenConfig,
        a: &mut Assembler,
        m: &mut Machine,
        loc: Location,
        target: Location,
        ret: Location,
        memarg: &MemoryImmediate,
        value_size: usize,
        memory_sz: Size,
        stack_sz: Size,
        cb: F,
    ) -> Result<(), CodegenError> {
        if memory_sz > stack_sz {
            return Err(CodegenError {
                message: format!("emit_compare_and_swap: memory size > stac size"),
            });
        }

        let compare = m.reserve_unused_temp_gpr(GPR::RAX);
        let value = if loc == Location::GPR(GPR::R14) {
            GPR::R13
        } else {
            GPR::R14
        };
        a.emit_push(Size::S64, Location::GPR(value));

        a.emit_mov(stack_sz, loc, Location::GPR(value));

        let retry = a.get_label();
        a.emit_label(retry);

        Self::emit_memory_op(
            module_info,
            config,
            a,
            m,
            target,
            memarg,
            true,
            value_size,
            |a, m, addr| {
                a.emit_mov(memory_sz, Location::Memory(addr, 0), Location::GPR(compare));
                a.emit_mov(stack_sz, Location::GPR(compare), ret);
                cb(a, m, compare, value);
                a.emit_lock_cmpxchg(memory_sz, Location::GPR(value), Location::Memory(addr, 0));
                Ok(())
            },
        )?;

        a.emit_jmp(Condition::NotEqual, retry);

        a.emit_pop(Size::S64, Location::GPR(value));
        m.release_temp_gpr(compare);
        Ok(())
    }

    // Checks for underflow/overflow/nan.
    fn emit_f32_int_conv_check(
        a: &mut Assembler,
        m: &mut Machine,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
        underflow_label: <Assembler as Emitter>::Label,
        overflow_label: <Assembler as Emitter>::Label,
        nan_label: <Assembler as Emitter>::Label,
        succeed_label: <Assembler as Emitter>::Label,
    ) {
        let lower_bound = f32::to_bits(lower_bound);
        let upper_bound = f32::to_bits(upper_bound);

        let tmp = m.acquire_temp_gpr().unwrap();
        let tmp_x = m.acquire_temp_xmm().unwrap();

        // Underflow.
        a.emit_mov(Size::S32, Location::Imm32(lower_bound), Location::GPR(tmp));
        a.emit_mov(Size::S32, Location::GPR(tmp), Location::XMM(tmp_x));
        a.emit_vcmpless(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        a.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        a.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        a.emit_jmp(Condition::NotEqual, underflow_label);

        // Overflow.
        a.emit_mov(Size::S32, Location::Imm32(upper_bound), Location::GPR(tmp));
        a.emit_mov(Size::S32, Location::GPR(tmp), Location::XMM(tmp_x));
        a.emit_vcmpgess(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        a.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        a.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        a.emit_jmp(Condition::NotEqual, overflow_label);

        // NaN.
        a.emit_vcmpeqss(reg, XMMOrMemory::XMM(reg), tmp_x);
        a.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        a.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        a.emit_jmp(Condition::Equal, nan_label);

        a.emit_jmp(Condition::None, succeed_label);

        m.release_temp_xmm(tmp_x);
        m.release_temp_gpr(tmp);
    }

    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F32.
    fn emit_f32_int_conv_check_trap(
        a: &mut Assembler,
        m: &mut Machine,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
    ) {
        let trap = a.get_label();
        let end = a.get_label();

        Self::emit_f32_int_conv_check(a, m, reg, lower_bound, upper_bound, trap, trap, trap, end);
        a.emit_label(trap);
        a.emit_ud2();
        a.emit_label(end);
    }

    fn emit_f32_int_conv_check_sat<
        F1: FnOnce(&mut Assembler, &mut Machine),
        F2: FnOnce(&mut Assembler, &mut Machine),
        F3: FnOnce(&mut Assembler, &mut Machine),
        F4: FnOnce(&mut Assembler, &mut Machine),
    >(
        a: &mut Assembler,
        m: &mut Machine,
        reg: XMM,
        lower_bound: f32,
        upper_bound: f32,
        underflow_cb: F1,
        overflow_cb: F2,
        nan_cb: Option<F3>,
        convert_cb: F4,
    ) {
        // As an optimization nan_cb is optional, and when set to None we turn
        // use 'underflow' as the 'nan' label. This is useful for callers who
        // set the return value to zero for both underflow and nan.

        let underflow = a.get_label();
        let overflow = a.get_label();
        let nan = if nan_cb.is_some() {
            a.get_label()
        } else {
            underflow
        };
        let convert = a.get_label();
        let end = a.get_label();

        Self::emit_f32_int_conv_check(
            a,
            m,
            reg,
            lower_bound,
            upper_bound,
            underflow,
            overflow,
            nan,
            convert,
        );

        a.emit_label(underflow);
        underflow_cb(a, m);
        a.emit_jmp(Condition::None, end);

        a.emit_label(overflow);
        overflow_cb(a, m);
        a.emit_jmp(Condition::None, end);

        if let Some(cb) = nan_cb {
            a.emit_label(nan);
            cb(a, m);
            a.emit_jmp(Condition::None, end);
        }

        a.emit_label(convert);
        convert_cb(a, m);
        a.emit_label(end);
    }

    // Checks for underflow/overflow/nan.
    fn emit_f64_int_conv_check(
        a: &mut Assembler,
        m: &mut Machine,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
        underflow_label: <Assembler as Emitter>::Label,
        overflow_label: <Assembler as Emitter>::Label,
        nan_label: <Assembler as Emitter>::Label,
        succeed_label: <Assembler as Emitter>::Label,
    ) {
        let lower_bound = f64::to_bits(lower_bound);
        let upper_bound = f64::to_bits(upper_bound);

        let tmp = m.acquire_temp_gpr().unwrap();
        let tmp_x = m.acquire_temp_xmm().unwrap();

        // Underflow.
        a.emit_mov(Size::S64, Location::Imm64(lower_bound), Location::GPR(tmp));
        a.emit_mov(Size::S64, Location::GPR(tmp), Location::XMM(tmp_x));
        a.emit_vcmplesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        a.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        a.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        a.emit_jmp(Condition::NotEqual, underflow_label);

        // Overflow.
        a.emit_mov(Size::S64, Location::Imm64(upper_bound), Location::GPR(tmp));
        a.emit_mov(Size::S64, Location::GPR(tmp), Location::XMM(tmp_x));
        a.emit_vcmpgesd(reg, XMMOrMemory::XMM(tmp_x), tmp_x);
        a.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        a.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        a.emit_jmp(Condition::NotEqual, overflow_label);

        // NaN.
        a.emit_vcmpeqsd(reg, XMMOrMemory::XMM(reg), tmp_x);
        a.emit_mov(Size::S32, Location::XMM(tmp_x), Location::GPR(tmp));
        a.emit_cmp(Size::S32, Location::Imm32(0), Location::GPR(tmp));
        a.emit_jmp(Condition::Equal, nan_label);

        a.emit_jmp(Condition::None, succeed_label);

        m.release_temp_xmm(tmp_x);
        m.release_temp_gpr(tmp);
    }

    // Checks for underflow/overflow/nan before IxxTrunc{U/S}F64.
    fn emit_f64_int_conv_check_trap(
        a: &mut Assembler,
        m: &mut Machine,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
    ) {
        let trap = a.get_label();
        let end = a.get_label();

        Self::emit_f64_int_conv_check(a, m, reg, lower_bound, upper_bound, trap, trap, trap, end);
        a.emit_label(trap);
        a.emit_ud2();
        a.emit_label(end);
    }

    fn emit_f64_int_conv_check_sat<
        F1: FnOnce(&mut Assembler, &mut Machine),
        F2: FnOnce(&mut Assembler, &mut Machine),
        F3: FnOnce(&mut Assembler, &mut Machine),
        F4: FnOnce(&mut Assembler, &mut Machine),
    >(
        a: &mut Assembler,
        m: &mut Machine,
        reg: XMM,
        lower_bound: f64,
        upper_bound: f64,
        underflow_cb: F1,
        overflow_cb: F2,
        nan_cb: Option<F3>,
        convert_cb: F4,
    ) {
        // As an optimization nan_cb is optional, and when set to None we turn
        // use 'underflow' as the 'nan' label. This is useful for callers who
        // set the return value to zero for both underflow and nan.

        let underflow = a.get_label();
        let overflow = a.get_label();
        let nan = if nan_cb.is_some() {
            a.get_label()
        } else {
            underflow
        };
        let convert = a.get_label();
        let end = a.get_label();

        Self::emit_f64_int_conv_check(
            a,
            m,
            reg,
            lower_bound,
            upper_bound,
            underflow,
            overflow,
            nan,
            convert,
        );

        a.emit_label(underflow);
        underflow_cb(a, m);
        a.emit_jmp(Condition::None, end);

        a.emit_label(overflow);
        overflow_cb(a, m);
        a.emit_jmp(Condition::None, end);

        if let Some(cb) = nan_cb {
            a.emit_label(nan);
            cb(a, m);
            a.emit_jmp(Condition::None, end);
        }

        a.emit_label(convert);
        convert_cb(a, m);
        a.emit_label(end);
    }

    pub fn get_state_diff(
        m: &Machine,
        fsm: &mut FunctionStateMap,
        control_stack: &mut [ControlFrame],
    ) -> usize {
        if !m.track_state {
            return usize::MAX;
        }
        let last_frame = control_stack.last_mut().unwrap();
        let mut diff = m.state.diff(&last_frame.state);
        diff.last = Some(last_frame.state_diff_id);
        let id = fsm.diffs.len();
        last_frame.state = m.state.clone();
        last_frame.state_diff_id = id;
        fsm.diffs.push(diff);
        id
    }
}

impl FunctionCodeGenerator<CodegenError> for X64FunctionCode {
    fn feed_return(&mut self, ty: WpType) -> Result<(), CodegenError> {
        self.returns.push(ty);
        Ok(())
    }

    fn feed_param(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        self.num_params += 1;
        self.num_locals += 1;
        Ok(())
    }

    fn feed_local(&mut self, _ty: WpType, n: usize) -> Result<(), CodegenError> {
        self.num_locals += n;
        Ok(())
    }

    fn begin_body(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        let a = self.assembler.as_mut().unwrap();
        let start_label = a.get_label();
        // skip the patchpoint during normal execution
        a.emit_jmp(Condition::None, start_label);
        // patchpoint of 32 1-byte nops
        for _ in 0..32 {
            a.emit_nop();
        }
        a.emit_label(start_label);
        a.emit_push(Size::S64, Location::GPR(GPR::RBP));
        a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RBP));

        // Stack check.
        if self.config.enforce_stack_check {
            a.emit_cmp(
                Size::S64,
                Location::Memory(
                    GPR::RDI, // first parameter is vmctx
                    vm::Ctx::offset_stack_lower_bound() as i32,
                ),
                Location::GPR(GPR::RSP),
            );
            a.emit_conditional_trap(Condition::Below);
        }

        self.locals = self
            .machine
            .init_locals(a, self.num_locals, self.num_params);

        self.machine.state.register_values
            [X64Register::GPR(Machine::get_vmctx_reg()).to_index().0] = MachineValue::Vmctx;

        self.fsm = FunctionStateMap::new(
            new_machine_state(),
            self.local_function_id,
            32,
            (0..self.locals.len())
                .map(|_| WasmAbstractValue::Runtime)
                .collect(),
        );

        let diff = self.machine.state.diff(&new_machine_state());
        let state_diff_id = self.fsm.diffs.len();
        self.fsm.diffs.push(diff);

        //println!("initial state = {:?}", self.machine.state);

        a.emit_sub(Size::S64, Location::Imm32(32), Location::GPR(GPR::RSP)); // simulate "red zone" if not supported by the platform

        self.control_stack.push(ControlFrame {
            label: a.get_label(),
            loop_like: false,
            if_else: IfElseState::None,
            returns: self.returns.clone(),
            value_stack_depth: 0,
            state: self.machine.state.clone(),
            state_diff_id,
        });

        // Check interrupt signal without branching
        let activate_offset = a.get_offset().0;

        a.emit_mov(
            Size::S64,
            Location::Memory(
                Machine::get_vmctx_reg(),
                vm::Ctx::offset_interrupt_signal_mem() as i32,
            ),
            Location::GPR(GPR::RAX),
        );
        self.fsm.loop_offsets.insert(
            a.get_offset().0,
            OffsetInfo {
                end_offset: a.get_offset().0 + 1,
                activate_offset,
                diff_id: state_diff_id,
            },
        );
        self.fsm.wasm_function_header_target_offset = Some(SuspendOffset::Loop(a.get_offset().0));
        a.emit_mov(
            Size::S64,
            Location::Memory(GPR::RAX, 0),
            Location::GPR(GPR::RAX),
        );

        if self.machine.state.wasm_inst_offset != usize::MAX {
            return Err(CodegenError {
                message: format!("begin_body: wasm_inst_offset not usize::MAX"),
            });
        }
        Ok(())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        let a = self.assembler.as_mut().unwrap();
        a.emit_ud2();
        Ok(())
    }

    fn feed_event(&mut self, ev: Event, module_info: &ModuleInfo) -> Result<(), CodegenError> {
        let a = self.assembler.as_mut().unwrap();

        match ev {
            Event::Internal(InternalEvent::FunctionBegin(_))
            | Event::Internal(InternalEvent::FunctionEnd) => {
                return Ok(());
            }
            _ => {}
        }

        self.machine.state.wasm_inst_offset = self.machine.state.wasm_inst_offset.wrapping_add(1);

        //println!("{:?} {}", op, self.value_stack.len());
        let was_unreachable;

        if self.unreachable_depth > 0 {
            was_unreachable = true;

            if let Event::Wasm(op) = ev {
                match *op {
                    Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                        self.unreachable_depth += 1;
                    }
                    Operator::End => {
                        self.unreachable_depth -= 1;
                    }
                    Operator::Else => {
                        // We are in a reachable true branch
                        if self.unreachable_depth == 1 {
                            if let Some(IfElseState::If(_)) =
                                self.control_stack.last().map(|x| x.if_else)
                            {
                                self.unreachable_depth -= 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if self.unreachable_depth > 0 {
                return Ok(());
            }
        } else {
            was_unreachable = false;
        }

        let op = match ev {
            Event::Wasm(x) => x,
            Event::WasmOwned(ref x) => x,
            Event::Internal(x) => {
                match x {
                    InternalEvent::Breakpoint(callback) => {
                        self.breakpoints
                            .as_mut()
                            .unwrap()
                            .insert(a.get_offset(), callback);
                        Self::mark_trappable(
                            a,
                            &self.machine,
                            &mut self.fsm,
                            &mut self.control_stack,
                        );
                        a.emit_inline_breakpoint(InlineBreakpointType::Middleware);
                    }
                    InternalEvent::FunctionBegin(_) | InternalEvent::FunctionEnd => {}
                    InternalEvent::GetInternal(idx) => {
                        let idx = idx as usize;
                        if idx >= INTERNALS_SIZE {
                            return Err(CodegenError {
                                message: format!("GetInternal: incorrect index value"),
                            });
                        }

                        let tmp = self.machine.acquire_temp_gpr().unwrap();

                        // Load `internals` pointer.
                        a.emit_mov(
                            Size::S64,
                            Location::Memory(
                                Machine::get_vmctx_reg(),
                                vm::Ctx::offset_internals() as i32,
                            ),
                            Location::GPR(tmp),
                        );

                        let loc = self.machine.acquire_locations(
                            a,
                            &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                            false,
                        )[0];
                        self.value_stack.push(loc);

                        // Move internal into the result location.
                        Self::emit_relaxed_binop(
                            a,
                            &mut self.machine,
                            Assembler::emit_mov,
                            Size::S64,
                            Location::Memory(tmp, (idx * 8) as i32),
                            loc,
                        );

                        self.machine.release_temp_gpr(tmp);
                    }
                    InternalEvent::SetInternal(idx) => {
                        let idx = idx as usize;
                        if idx >= INTERNALS_SIZE {
                            return Err(CodegenError {
                                message: format!("SetInternal: incorrect index value"),
                            });
                        }

                        let tmp = self.machine.acquire_temp_gpr().unwrap();

                        // Load `internals` pointer.
                        a.emit_mov(
                            Size::S64,
                            Location::Memory(
                                Machine::get_vmctx_reg(),
                                vm::Ctx::offset_internals() as i32,
                            ),
                            Location::GPR(tmp),
                        );
                        let loc = get_location_released(
                            a,
                            &mut self.machine,
                            self.value_stack.pop().unwrap(),
                        );

                        // Move internal into storage.
                        Self::emit_relaxed_binop(
                            a,
                            &mut self.machine,
                            Assembler::emit_mov,
                            Size::S64,
                            loc,
                            Location::Memory(tmp, (idx * 8) as i32),
                        );
                        self.machine.release_temp_gpr(tmp);
                    } //_ => unimplemented!(),
                }
                return Ok(());
            }
        };

        match *op {
            Operator::GlobalGet { global_index } => {
                let global_index = global_index as usize;

                let tmp = self.machine.acquire_temp_gpr().unwrap();

                let loc = match GlobalIndex::new(global_index).local_or_import(module_info) {
                    LocalOrImport::Local(local_index) => {
                        a.emit_mov(
                            Size::S64,
                            Location::Memory(
                                Machine::get_vmctx_reg(),
                                vm::Ctx::offset_globals() as i32,
                            ),
                            Location::GPR(tmp),
                        );
                        a.emit_mov(
                            Size::S64,
                            Location::Memory(tmp, (local_index.index() as i32) * 8),
                            Location::GPR(tmp),
                        );
                        self.machine.acquire_locations(
                            a,
                            &[(
                                type_to_wp_type(module_info.globals[local_index].desc.ty),
                                MachineValue::WasmStack(self.value_stack.len()),
                            )],
                            false,
                        )[0]
                    }
                    LocalOrImport::Import(import_index) => {
                        a.emit_mov(
                            Size::S64,
                            Location::Memory(
                                Machine::get_vmctx_reg(),
                                vm::Ctx::offset_imported_globals() as i32,
                            ),
                            Location::GPR(tmp),
                        );
                        a.emit_mov(
                            Size::S64,
                            Location::Memory(tmp, (import_index.index() as i32) * 8),
                            Location::GPR(tmp),
                        );
                        self.machine.acquire_locations(
                            a,
                            &[(
                                type_to_wp_type(module_info.imported_globals[import_index].1.ty),
                                MachineValue::WasmStack(self.value_stack.len()),
                            )],
                            false,
                        )[0]
                    }
                };
                self.value_stack.push(loc);

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    Location::Memory(tmp, LocalGlobal::offset_data() as i32),
                    loc,
                );

                self.machine.release_temp_gpr(tmp);
            }
            Operator::GlobalSet { global_index } => {
                let mut global_index = global_index as usize;
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                let tmp = self.machine.acquire_temp_gpr().unwrap();

                if global_index < module_info.imported_globals.len() {
                    a.emit_mov(
                        Size::S64,
                        Location::Memory(
                            Machine::get_vmctx_reg(),
                            vm::Ctx::offset_imported_globals() as i32,
                        ),
                        Location::GPR(tmp),
                    );
                } else {
                    global_index -= module_info.imported_globals.len();
                    if global_index >= module_info.globals.len() {
                        return Err(CodegenError {
                            message: format!("SetGlobal: incorrect global_index value"),
                        });
                    }
                    a.emit_mov(
                        Size::S64,
                        Location::Memory(
                            Machine::get_vmctx_reg(),
                            vm::Ctx::offset_globals() as i32,
                        ),
                        Location::GPR(tmp),
                    );
                }
                a.emit_mov(
                    Size::S64,
                    Location::Memory(tmp, (global_index as i32) * 8),
                    Location::GPR(tmp),
                );
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    loc,
                    Location::Memory(tmp, LocalGlobal::offset_data() as i32),
                );

                self.machine.release_temp_gpr(tmp);
            }
            Operator::LocalGet { local_index } => {
                let local_index = local_index as usize;
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    self.locals[local_index],
                    ret,
                );
                self.value_stack.push(ret);
            }
            Operator::LocalSet { local_index } => {
                let local_index = local_index as usize;
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    loc,
                    self.locals[local_index],
                );
            }
            Operator::LocalTee { local_index } => {
                let local_index = local_index as usize;
                let loc = *self.value_stack.last().unwrap();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    loc,
                    self.locals[local_index],
                );
            }
            Operator::I32Const { value } => {
                self.value_stack.push(Location::Imm32(value as u32));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value as u32 as u64));
            }
            Operator::I32Add => Self::emit_binop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_add,
            ),
            Operator::I32Sub => Self::emit_binop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_sub,
            ),
            Operator::I32Mul => Self::emit_binop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_imul,
            ),
            Operator::I32DivU => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_div,
                    Size::S32,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S32, Location::GPR(GPR::RAX), ret);
                self.value_stack.push(ret);
            }
            Operator::I32DivS => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_cdq();
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_idiv,
                    Size::S32,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S32, Location::GPR(GPR::RAX), ret);
                self.value_stack.push(ret);
            }
            Operator::I32RemU => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_div,
                    Size::S32,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S32, Location::GPR(GPR::RDX), ret);
                self.value_stack.push(ret);
            }
            Operator::I32RemS => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];

                let normal_path = a.get_label();
                let end = a.get_label();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S32,
                    Location::Imm32(0x80000000),
                    loc_a,
                );
                a.emit_jmp(Condition::NotEqual, normal_path);
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S32,
                    Location::Imm32(0xffffffff),
                    loc_b,
                );
                a.emit_jmp(Condition::NotEqual, normal_path);
                a.emit_mov(Size::S32, Location::Imm32(0), ret);
                a.emit_jmp(Condition::None, end);

                a.emit_label(normal_path);
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_cdq();
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_idiv,
                    Size::S32,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S32, Location::GPR(GPR::RDX), ret);
                self.value_stack.push(ret);

                a.emit_label(end);
            }
            Operator::I32And => Self::emit_binop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_and,
            ),
            Operator::I32Or => Self::emit_binop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_or,
            ),
            Operator::I32Xor => Self::emit_binop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_xor,
            ),
            Operator::I32Eq => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Equal,
            )?,
            Operator::I32Ne => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::NotEqual,
            )?,
            Operator::I32Eqz => Self::emit_cmpop_i32_dynamic_b(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Equal,
                Location::Imm32(0),
            )?,
            Operator::I32Clz => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let src = match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        a.emit_mov(Size::S32, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I32Clz src: unreachable code"),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I32Clz dst: unreachable code"),
                        })
                    }
                };

                if a.arch_has_xzcnt() {
                    a.arch_emit_lzcnt(Size::S32, Location::GPR(src), Location::GPR(dst));
                } else {
                    let zero_path = a.get_label();
                    let end = a.get_label();

                    a.emit_test_gpr_64(src);
                    a.emit_jmp(Condition::Equal, zero_path);
                    a.emit_bsr(Size::S32, Location::GPR(src), Location::GPR(dst));
                    a.emit_xor(Size::S32, Location::Imm32(31), Location::GPR(dst));
                    a.emit_jmp(Condition::None, end);
                    a.emit_label(zero_path);
                    a.emit_mov(Size::S32, Location::Imm32(32), Location::GPR(dst));
                    a.emit_label(end);
                }

                match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                match ret {
                    Location::Memory(_, _) => {
                        a.emit_mov(Size::S32, Location::GPR(dst), ret);
                        self.machine.release_temp_gpr(dst);
                    }
                    _ => {}
                };
            }
            Operator::I32Ctz => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let src = match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        a.emit_mov(Size::S32, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I32Ctz src: unreachable code"),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I32Ctz dst: unreachable code"),
                        })
                    }
                };

                if a.arch_has_xzcnt() {
                    a.arch_emit_tzcnt(Size::S32, Location::GPR(src), Location::GPR(dst));
                } else {
                    let zero_path = a.get_label();
                    let end = a.get_label();

                    a.emit_test_gpr_64(src);
                    a.emit_jmp(Condition::Equal, zero_path);
                    a.emit_bsf(Size::S32, Location::GPR(src), Location::GPR(dst));
                    a.emit_jmp(Condition::None, end);
                    a.emit_label(zero_path);
                    a.emit_mov(Size::S32, Location::Imm32(32), Location::GPR(dst));
                    a.emit_label(end);
                }

                match loc {
                    Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                match ret {
                    Location::Memory(_, _) => {
                        a.emit_mov(Size::S32, Location::GPR(dst), ret);
                        self.machine.release_temp_gpr(dst);
                    }
                    _ => {}
                };
            }
            Operator::I32Popcnt => Self::emit_xcnt_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_popcnt,
            )?,
            Operator::I32Shl => Self::emit_shift_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_shl,
            ),
            Operator::I32ShrU => Self::emit_shift_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_shr,
            ),
            Operator::I32ShrS => Self::emit_shift_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_sar,
            ),
            Operator::I32Rotl => Self::emit_shift_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_rol,
            ),
            Operator::I32Rotr => Self::emit_shift_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_ror,
            ),
            Operator::I32LtU => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Below,
            )?,
            Operator::I32LeU => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::BelowEqual,
            )?,
            Operator::I32GtU => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Above,
            )?,
            Operator::I32GeU => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::AboveEqual,
            )?,
            Operator::I32LtS => {
                Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Less)?;
            }
            Operator::I32LeS => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::LessEqual,
            )?,
            Operator::I32GtS => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Greater,
            )?,
            Operator::I32GeS => Self::emit_cmpop_i32(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::GreaterEqual,
            )?,
            Operator::I64Const { value } => {
                let value = value as u64;
                self.value_stack.push(Location::Imm64(value));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value));
            }
            Operator::I64Add => Self::emit_binop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_add,
            ),
            Operator::I64Sub => Self::emit_binop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_sub,
            ),
            Operator::I64Mul => Self::emit_binop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_imul,
            ),
            Operator::I64DivU => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                a.emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S64, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_div,
                    Size::S64,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S64, Location::GPR(GPR::RAX), ret);
                self.value_stack.push(ret);
            }
            Operator::I64DivS => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                a.emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                a.emit_cqo();
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_idiv,
                    Size::S64,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S64, Location::GPR(GPR::RAX), ret);
                self.value_stack.push(ret);
            }
            Operator::I64RemU => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                a.emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S64, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_div,
                    Size::S64,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S64, Location::GPR(GPR::RDX), ret);
                self.value_stack.push(ret);
            }
            Operator::I64RemS => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];

                let normal_path = a.get_label();
                let end = a.get_label();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S64,
                    Location::Imm64(0x8000000000000000u64),
                    loc_a,
                );
                a.emit_jmp(Condition::NotEqual, normal_path);
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S64,
                    Location::Imm64(0xffffffffffffffffu64),
                    loc_b,
                );
                a.emit_jmp(Condition::NotEqual, normal_path);
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    Location::Imm64(0),
                    ret,
                );
                a.emit_jmp(Condition::None, end);

                a.emit_label(normal_path);

                a.emit_mov(Size::S64, loc_a, Location::GPR(GPR::RAX));
                a.emit_cqo();
                Self::emit_relaxed_xdiv(
                    a,
                    &mut self.machine,
                    Assembler::emit_idiv,
                    Size::S64,
                    loc_b,
                    &mut self.fsm,
                    &mut self.control_stack,
                );
                a.emit_mov(Size::S64, Location::GPR(GPR::RDX), ret);
                self.value_stack.push(ret);
                a.emit_label(end);
            }
            Operator::I64And => Self::emit_binop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_and,
            ),
            Operator::I64Or => Self::emit_binop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_or,
            ),
            Operator::I64Xor => Self::emit_binop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_xor,
            ),
            Operator::I64Eq => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Equal,
            )?,
            Operator::I64Ne => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::NotEqual,
            )?,
            Operator::I64Eqz => Self::emit_cmpop_i64_dynamic_b(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Equal,
                Location::Imm64(0),
            )?,
            Operator::I64Clz => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let src = match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        a.emit_mov(Size::S64, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I64Clz src: unreachable code"),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I64Clz dst: unreachable code"),
                        })
                    }
                };

                if a.arch_has_xzcnt() {
                    a.arch_emit_lzcnt(Size::S64, Location::GPR(src), Location::GPR(dst));
                } else {
                    let zero_path = a.get_label();
                    let end = a.get_label();

                    a.emit_test_gpr_64(src);
                    a.emit_jmp(Condition::Equal, zero_path);
                    a.emit_bsr(Size::S64, Location::GPR(src), Location::GPR(dst));
                    a.emit_xor(Size::S64, Location::Imm32(63), Location::GPR(dst));
                    a.emit_jmp(Condition::None, end);
                    a.emit_label(zero_path);
                    a.emit_mov(Size::S64, Location::Imm32(64), Location::GPR(dst));
                    a.emit_label(end);
                }

                match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                match ret {
                    Location::Memory(_, _) => {
                        a.emit_mov(Size::S64, Location::GPR(dst), ret);
                        self.machine.release_temp_gpr(dst);
                    }
                    _ => {}
                };
            }
            Operator::I64Ctz => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let src = match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        let tmp = self.machine.acquire_temp_gpr().unwrap();
                        a.emit_mov(Size::S64, loc, Location::GPR(tmp));
                        tmp
                    }
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I64Ctz src: unreachable code"),
                        })
                    }
                };

                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let dst = match ret {
                    Location::Memory(_, _) => self.machine.acquire_temp_gpr().unwrap(),
                    Location::GPR(reg) => reg,
                    _ => {
                        return Err(CodegenError {
                            message: format!("I64Ctz dst: unreachable code"),
                        })
                    }
                };

                if a.arch_has_xzcnt() {
                    a.arch_emit_tzcnt(Size::S64, Location::GPR(src), Location::GPR(dst));
                } else {
                    let zero_path = a.get_label();
                    let end = a.get_label();

                    a.emit_test_gpr_64(src);
                    a.emit_jmp(Condition::Equal, zero_path);
                    a.emit_bsf(Size::S64, Location::GPR(src), Location::GPR(dst));
                    a.emit_jmp(Condition::None, end);
                    a.emit_label(zero_path);
                    a.emit_mov(Size::S64, Location::Imm32(64), Location::GPR(dst));
                    a.emit_label(end);
                }

                match loc {
                    Location::Imm64(_) | Location::Imm32(_) | Location::Memory(_, _) => {
                        self.machine.release_temp_gpr(src);
                    }
                    _ => {}
                };
                match ret {
                    Location::Memory(_, _) => {
                        a.emit_mov(Size::S64, Location::GPR(dst), ret);
                        self.machine.release_temp_gpr(dst);
                    }
                    _ => {}
                };
            }
            Operator::I64Popcnt => Self::emit_xcnt_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_popcnt,
            )?,
            Operator::I64Shl => Self::emit_shift_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_shl,
            ),
            Operator::I64ShrU => Self::emit_shift_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_shr,
            ),
            Operator::I64ShrS => Self::emit_shift_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_sar,
            ),
            Operator::I64Rotl => Self::emit_shift_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_rol,
            ),
            Operator::I64Rotr => Self::emit_shift_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_ror,
            ),
            Operator::I64LtU => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Below,
            )?,
            Operator::I64LeU => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::BelowEqual,
            )?,
            Operator::I64GtU => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Above,
            )?,
            Operator::I64GeU => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::AboveEqual,
            )?,
            Operator::I64LtS => {
                Self::emit_cmpop_i64(a, &mut self.machine, &mut self.value_stack, Condition::Less)?;
            }
            Operator::I64LeS => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::LessEqual,
            )?,
            Operator::I64GtS => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::Greater,
            )?,
            Operator::I64GeS => Self::emit_cmpop_i64(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Condition::GreaterEqual,
            )?,
            Operator::I64ExtendI32U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S32,
                    loc,
                    ret,
                );
            }
            Operator::I64ExtendI32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                Self::emit_relaxed_zx_sx(
                    a,
                    &mut self.machine,
                    Assembler::emit_movsx,
                    Size::S32,
                    loc,
                    Size::S64,
                    ret,
                )?;
            }
            Operator::I32Extend8S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_relaxed_zx_sx(
                    a,
                    &mut self.machine,
                    Assembler::emit_movsx,
                    Size::S8,
                    loc,
                    Size::S32,
                    ret,
                )?;
            }
            Operator::I32Extend16S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_relaxed_zx_sx(
                    a,
                    &mut self.machine,
                    Assembler::emit_movsx,
                    Size::S16,
                    loc,
                    Size::S32,
                    ret,
                )?;
            }
            Operator::I64Extend8S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_relaxed_zx_sx(
                    a,
                    &mut self.machine,
                    Assembler::emit_movsx,
                    Size::S8,
                    loc,
                    Size::S64,
                    ret,
                )?;
            }
            Operator::I64Extend16S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_relaxed_zx_sx(
                    a,
                    &mut self.machine,
                    Assembler::emit_movsx,
                    Size::S16,
                    loc,
                    Size::S64,
                    ret,
                )?;
            }
            Operator::I64Extend32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_relaxed_zx_sx(
                    a,
                    &mut self.machine,
                    Assembler::emit_movsx,
                    Size::S32,
                    loc,
                    Size::S64,
                    ret,
                )?;
            }
            Operator::I32WrapI64 => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S32,
                    loc,
                    ret,
                );
            }

            Operator::F32Const { value } => {
                self.value_stack.push(Location::Imm32(value.bits()));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value.bits() as u64));
            }
            Operator::F32Add => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vaddss,
            )?,
            Operator::F32Sub => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vsubss,
            )?,
            Operator::F32Mul => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vmulss,
            )?,
            Operator::F32Div => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vdivss,
            )?,
            Operator::F32Max => {
                if !a.arch_supports_canonicalize_nan() {
                    Self::emit_fp_binop_avx(
                        a,
                        &mut self.machine,
                        &mut self.value_stack,
                        Assembler::emit_vmaxss,
                    )?;
                } else {
                    let src2 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let src1 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let ret = self.machine.acquire_locations(
                        a,
                        &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                        false,
                    )[0];
                    self.value_stack.push(ret);

                    let tmp1 = self.machine.acquire_temp_xmm().unwrap();
                    let tmp2 = self.machine.acquire_temp_xmm().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match src1 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src1, Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F32Max src1: unreachable code"),
                            })
                        }
                    };
                    let src2 = match src2 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src2, Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F32Max src2: unreachable code"),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    a.emit_mov(Size::S32, Location::XMM(src1), Location::GPR(tmpg1));
                    a.emit_mov(Size::S32, Location::XMM(src2), Location::GPR(tmpg2));
                    a.emit_cmp(Size::S32, Location::GPR(tmpg2), Location::GPR(tmpg1));
                    a.emit_vmaxss(src1, XMMOrMemory::XMM(src2), tmp_xmm1);
                    let label1 = a.get_label();
                    let label2 = a.get_label();
                    a.emit_jmp(Condition::NotEqual, label1);
                    a.emit_vmovaps(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    a.emit_jmp(Condition::None, label2);
                    a.emit_label(label1);
                    a.emit_vxorps(tmp_xmm2, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm2);
                    a.emit_label(label2);
                    a.emit_vcmpeqss(src1, XMMOrMemory::XMM(src2), tmp_xmm3);
                    a.emit_vblendvps(tmp_xmm3, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm1, tmp_xmm1);
                    a.emit_vcmpunordss(src1, XMMOrMemory::XMM(src2), src1);
                    // load float canonical nan
                    a.emit_mov(
                        Size::S64,
                        Location::Imm32(0x7FC0_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(src2));
                    a.emit_vblendvps(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1);
                    match ret {
                        Location::XMM(x) => {
                            a.emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            a.emit_mov(Size::S64, Location::XMM(src1), ret);
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F32Max ret: unreachable code"),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_xmm(tmp2);
                    self.machine.release_temp_xmm(tmp1);
                }
            }
            Operator::F32Min => {
                if !a.arch_supports_canonicalize_nan() {
                    Self::emit_fp_binop_avx(
                        a,
                        &mut self.machine,
                        &mut self.value_stack,
                        Assembler::emit_vminss,
                    )?;
                } else {
                    let src2 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let src1 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let ret = self.machine.acquire_locations(
                        a,
                        &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                        false,
                    )[0];
                    self.value_stack.push(ret);

                    let tmp1 = self.machine.acquire_temp_xmm().unwrap();
                    let tmp2 = self.machine.acquire_temp_xmm().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match src1 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src1, Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F32Min src1: unreachable code"),
                            })
                        }
                    };
                    let src2 = match src2 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src2, Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F32Min src2: unreachable code"),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    a.emit_mov(Size::S32, Location::XMM(src1), Location::GPR(tmpg1));
                    a.emit_mov(Size::S32, Location::XMM(src2), Location::GPR(tmpg2));
                    a.emit_cmp(Size::S32, Location::GPR(tmpg2), Location::GPR(tmpg1));
                    a.emit_vminss(src1, XMMOrMemory::XMM(src2), tmp_xmm1);
                    let label1 = a.get_label();
                    let label2 = a.get_label();
                    a.emit_jmp(Condition::NotEqual, label1);
                    a.emit_vmovaps(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    a.emit_jmp(Condition::None, label2);
                    a.emit_label(label1);
                    // load float -0.0
                    a.emit_mov(
                        Size::S64,
                        Location::Imm32(0x8000_0000), // Negative zero
                        Location::GPR(tmpg1),
                    );
                    a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp_xmm2));
                    a.emit_label(label2);
                    a.emit_vcmpeqss(src1, XMMOrMemory::XMM(src2), tmp_xmm3);
                    a.emit_vblendvps(tmp_xmm3, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm1, tmp_xmm1);
                    a.emit_vcmpunordss(src1, XMMOrMemory::XMM(src2), src1);
                    // load float canonical nan
                    a.emit_mov(
                        Size::S64,
                        Location::Imm32(0x7FC0_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(src2));
                    a.emit_vblendvps(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1);
                    match ret {
                        Location::XMM(x) => {
                            a.emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            a.emit_mov(Size::S64, Location::XMM(src1), ret);
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F32Min ret: unreachable code"),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_xmm(tmp2);
                    self.machine.release_temp_xmm(tmp1);
                }
            }
            Operator::F32Eq => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpeqss,
            )?,
            Operator::F32Ne => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpneqss,
            )?,
            Operator::F32Lt => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpltss,
            )?,
            Operator::F32Le => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpless,
            )?,
            Operator::F32Gt => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpgtss,
            )?,
            Operator::F32Ge => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpgess,
            )?,
            Operator::F32Nearest => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundss_nearest,
            )?,
            Operator::F32Floor => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundss_floor,
            )?,
            Operator::F32Ceil => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundss_ceil,
            )?,
            Operator::F32Trunc => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundss_trunc,
            )?,
            Operator::F32Sqrt => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vsqrtss,
            )?,

            Operator::F32Copysign => {
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc_a, Location::GPR(tmp1));
                a.emit_mov(Size::S32, loc_b, Location::GPR(tmp2));
                a.emit_and(
                    Size::S32,
                    Location::Imm32(0x7fffffffu32),
                    Location::GPR(tmp1),
                );
                a.emit_and(
                    Size::S32,
                    Location::Imm32(0x80000000u32),
                    Location::GPR(tmp2),
                );
                a.emit_or(Size::S32, Location::GPR(tmp2), Location::GPR(tmp1));
                a.emit_mov(Size::S32, Location::GPR(tmp1), ret);
                self.machine.release_temp_gpr(tmp2);
                self.machine.release_temp_gpr(tmp1);
            }

            Operator::F32Abs => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                let tmp = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(tmp));
                a.emit_and(
                    Size::S32,
                    Location::Imm32(0x7fffffffu32),
                    Location::GPR(tmp),
                );
                a.emit_mov(Size::S32, Location::GPR(tmp), ret);
                self.machine.release_temp_gpr(tmp);
            }

            Operator::F32Neg => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_fneg() {
                    let tmp = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp),
                    );
                    a.arch_emit_f32_neg(tmp, tmp);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::XMM(tmp),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp);
                } else {
                    let tmp = self.machine.acquire_temp_gpr().unwrap();
                    a.emit_mov(Size::S32, loc, Location::GPR(tmp));
                    a.emit_btc_gpr_imm8_32(31, tmp);
                    a.emit_mov(Size::S32, Location::GPR(tmp), ret);
                    self.machine.release_temp_gpr(tmp);
                }
            }

            Operator::F64Const { value } => {
                self.value_stack.push(Location::Imm64(value.bits()));
                self.machine
                    .state
                    .wasm_stack
                    .push(WasmAbstractValue::Const(value.bits()));
            }
            Operator::F64Add => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vaddsd,
            )?,
            Operator::F64Sub => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vsubsd,
            )?,
            Operator::F64Mul => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vmulsd,
            )?,
            Operator::F64Div => Self::emit_fp_binop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vdivsd,
            )?,
            Operator::F64Max => {
                if !a.arch_supports_canonicalize_nan() {
                    Self::emit_fp_binop_avx(
                        a,
                        &mut self.machine,
                        &mut self.value_stack,
                        Assembler::emit_vmaxsd,
                    )?;
                } else {
                    let src2 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let src1 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let ret = self.machine.acquire_locations(
                        a,
                        &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                        false,
                    )[0];
                    self.value_stack.push(ret);

                    let tmp1 = self.machine.acquire_temp_xmm().unwrap();
                    let tmp2 = self.machine.acquire_temp_xmm().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match src1 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src1, Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F64Max src1: unreachable code"),
                            })
                        }
                    };
                    let src2 = match src2 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src2, Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F64Max src2: unreachable code"),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    a.emit_mov(Size::S64, Location::XMM(src1), Location::GPR(tmpg1));
                    a.emit_mov(Size::S64, Location::XMM(src2), Location::GPR(tmpg2));
                    a.emit_cmp(Size::S64, Location::GPR(tmpg2), Location::GPR(tmpg1));
                    a.emit_vmaxsd(src1, XMMOrMemory::XMM(src2), tmp_xmm1);
                    let label1 = a.get_label();
                    let label2 = a.get_label();
                    a.emit_jmp(Condition::NotEqual, label1);
                    a.emit_vmovapd(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    a.emit_jmp(Condition::None, label2);
                    a.emit_label(label1);
                    a.emit_vxorpd(tmp_xmm2, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm2);
                    a.emit_label(label2);
                    a.emit_vcmpeqsd(src1, XMMOrMemory::XMM(src2), tmp_xmm3);
                    a.emit_vblendvpd(tmp_xmm3, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm1, tmp_xmm1);
                    a.emit_vcmpunordsd(src1, XMMOrMemory::XMM(src2), src1);
                    // load float canonical nan
                    a.emit_mov(
                        Size::S64,
                        Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(src2));
                    a.emit_vblendvpd(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1);
                    match ret {
                        Location::XMM(x) => {
                            a.emit_vmovapd(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            a.emit_mov(Size::S64, Location::XMM(src1), ret);
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F64Max ret: unreachable code"),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_xmm(tmp2);
                    self.machine.release_temp_xmm(tmp1);
                }
            }
            Operator::F64Min => {
                if !a.arch_supports_canonicalize_nan() {
                    Self::emit_fp_binop_avx(
                        a,
                        &mut self.machine,
                        &mut self.value_stack,
                        Assembler::emit_vminsd,
                    )?;
                } else {
                    let src2 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let src1 = get_location_released(
                        a,
                        &mut self.machine,
                        self.value_stack.pop().unwrap(),
                    );
                    let ret = self.machine.acquire_locations(
                        a,
                        &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                        false,
                    )[0];
                    self.value_stack.push(ret);

                    let tmp1 = self.machine.acquire_temp_xmm().unwrap();
                    let tmp2 = self.machine.acquire_temp_xmm().unwrap();
                    let tmpg1 = self.machine.acquire_temp_gpr().unwrap();
                    let tmpg2 = self.machine.acquire_temp_gpr().unwrap();

                    let src1 = match src1 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src1, Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src1, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp1));
                            tmp1
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F64Min src1: unreachable code"),
                            })
                        }
                    };
                    let src2 = match src2 {
                        Location::XMM(x) => x,
                        Location::GPR(_) | Location::Memory(_, _) => {
                            a.emit_mov(Size::S64, src2, Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm32(_) => {
                            a.emit_mov(Size::S32, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S32, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        Location::Imm64(_) => {
                            a.emit_mov(Size::S64, src2, Location::GPR(tmpg1));
                            a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp2));
                            tmp2
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F64Min src2: unreachable code"),
                            })
                        }
                    };

                    let tmp_xmm1 = XMM::XMM8;
                    let tmp_xmm2 = XMM::XMM9;
                    let tmp_xmm3 = XMM::XMM10;

                    a.emit_mov(Size::S64, Location::XMM(src1), Location::GPR(tmpg1));
                    a.emit_mov(Size::S64, Location::XMM(src2), Location::GPR(tmpg2));
                    a.emit_cmp(Size::S64, Location::GPR(tmpg2), Location::GPR(tmpg1));
                    a.emit_vminsd(src1, XMMOrMemory::XMM(src2), tmp_xmm1);
                    let label1 = a.get_label();
                    let label2 = a.get_label();
                    a.emit_jmp(Condition::NotEqual, label1);
                    a.emit_vmovapd(XMMOrMemory::XMM(tmp_xmm1), XMMOrMemory::XMM(tmp_xmm2));
                    a.emit_jmp(Condition::None, label2);
                    a.emit_label(label1);
                    // load float -0.0
                    a.emit_mov(
                        Size::S64,
                        Location::Imm64(0x8000_0000_0000_0000), // Negative zero
                        Location::GPR(tmpg1),
                    );
                    a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(tmp_xmm2));
                    a.emit_label(label2);
                    a.emit_vcmpeqsd(src1, XMMOrMemory::XMM(src2), tmp_xmm3);
                    a.emit_vblendvpd(tmp_xmm3, XMMOrMemory::XMM(tmp_xmm2), tmp_xmm1, tmp_xmm1);
                    a.emit_vcmpunordsd(src1, XMMOrMemory::XMM(src2), src1);
                    // load float canonical nan
                    a.emit_mov(
                        Size::S64,
                        Location::Imm64(0x7FF8_0000_0000_0000), // Canonical NaN
                        Location::GPR(tmpg1),
                    );
                    a.emit_mov(Size::S64, Location::GPR(tmpg1), Location::XMM(src2));
                    a.emit_vblendvpd(src1, XMMOrMemory::XMM(src2), tmp_xmm1, src1);
                    match ret {
                        Location::XMM(x) => {
                            a.emit_vmovaps(XMMOrMemory::XMM(src1), XMMOrMemory::XMM(x));
                        }
                        Location::Memory(_, _) | Location::GPR(_) => {
                            a.emit_mov(Size::S64, Location::XMM(src1), ret);
                        }
                        _ => {
                            return Err(CodegenError {
                                message: format!("F64Min ret: unreachable code"),
                            })
                        }
                    }

                    self.machine.release_temp_gpr(tmpg2);
                    self.machine.release_temp_gpr(tmpg1);
                    self.machine.release_temp_xmm(tmp2);
                    self.machine.release_temp_xmm(tmp1);
                }
            }
            Operator::F64Eq => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpeqsd,
            )?,
            Operator::F64Ne => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpneqsd,
            )?,
            Operator::F64Lt => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpltsd,
            )?,
            Operator::F64Le => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmplesd,
            )?,
            Operator::F64Gt => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpgtsd,
            )?,
            Operator::F64Ge => Self::emit_fp_cmpop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcmpgesd,
            )?,
            Operator::F64Nearest => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundsd_nearest,
            )?,
            Operator::F64Floor => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundsd_floor,
            )?,
            Operator::F64Ceil => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundsd_ceil,
            )?,
            Operator::F64Trunc => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vroundsd_trunc,
            )?,
            Operator::F64Sqrt => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vsqrtsd,
            )?,

            Operator::F64Copysign => {
                let loc_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp1 = self.machine.acquire_temp_gpr().unwrap();
                let tmp2 = self.machine.acquire_temp_gpr().unwrap();
                let c = self.machine.acquire_temp_gpr().unwrap();

                a.emit_mov(Size::S64, loc_a, Location::GPR(tmp1));
                a.emit_mov(Size::S64, loc_b, Location::GPR(tmp2));

                a.emit_mov(
                    Size::S64,
                    Location::Imm64(0x7fffffffffffffffu64),
                    Location::GPR(c),
                );
                a.emit_and(Size::S64, Location::GPR(c), Location::GPR(tmp1));

                a.emit_mov(
                    Size::S64,
                    Location::Imm64(0x8000000000000000u64),
                    Location::GPR(c),
                );
                a.emit_and(Size::S64, Location::GPR(c), Location::GPR(tmp2));

                a.emit_or(Size::S64, Location::GPR(tmp2), Location::GPR(tmp1));
                a.emit_mov(Size::S64, Location::GPR(tmp1), ret);

                self.machine.release_temp_gpr(c);
                self.machine.release_temp_gpr(tmp2);
                self.machine.release_temp_gpr(tmp1);
            }

            Operator::F64Abs => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp = self.machine.acquire_temp_gpr().unwrap();
                let c = self.machine.acquire_temp_gpr().unwrap();

                a.emit_mov(Size::S64, loc, Location::GPR(tmp));
                a.emit_mov(
                    Size::S64,
                    Location::Imm64(0x7fffffffffffffffu64),
                    Location::GPR(c),
                );
                a.emit_and(Size::S64, Location::GPR(c), Location::GPR(tmp));
                a.emit_mov(Size::S64, Location::GPR(tmp), ret);

                self.machine.release_temp_gpr(c);
                self.machine.release_temp_gpr(tmp);
            }

            Operator::F64Neg => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                if a.arch_has_fneg() {
                    let tmp = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp),
                    );
                    a.arch_emit_f64_neg(tmp, tmp);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::XMM(tmp),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp);
                } else {
                    let tmp = self.machine.acquire_temp_gpr().unwrap();
                    a.emit_mov(Size::S64, loc, Location::GPR(tmp));
                    a.emit_btc_gpr_imm8_64(63, tmp);
                    a.emit_mov(Size::S64, Location::GPR(tmp), ret);
                    self.machine.release_temp_gpr(tmp);
                }
            }

            Operator::F64PromoteF32 => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcvtss2sd,
            )?,
            Operator::F32DemoteF64 => Self::emit_fp_unop_avx(
                a,
                &mut self.machine,
                &mut self.value_stack,
                Assembler::emit_vcvtsd2ss,
            )?,

            Operator::I32ReinterpretF32 => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if loc != ret {
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        ret,
                    );
                }
            }
            Operator::F32ReinterpretI32 => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if loc != ret {
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        ret,
                    );
                }
            }

            Operator::I64ReinterpretF64 => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if loc != ret {
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        ret,
                    );
                }
            }
            Operator::F64ReinterpretI64 => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if loc != ret {
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        ret,
                    );
                }
            }

            Operator::I32TruncF32U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i32_trunc_uf32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    Self::emit_f32_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        tmp_in,
                        GEF32_LT_U32_MIN,
                        LEF32_GT_U32_MAX,
                    );

                    a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I32TruncSatF32U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S32,
                    loc,
                    Location::XMM(tmp_in),
                );
                Self::emit_f32_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    tmp_in,
                    GEF32_LT_U32_MIN,
                    LEF32_GT_U32_MAX,
                    |a, _m| {
                        a.emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out));
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S32,
                            Location::Imm32(std::u32::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(_a: &mut Assembler, _m: &mut Machine)>,
                    |a, _m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i32_trunc_uf32(tmp_in, tmp_out);
                        } else {
                            a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I32TruncF32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i32_trunc_sf32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    Self::emit_f32_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        tmp_in,
                        GEF32_LT_I32_MIN,
                        LEF32_GT_I32_MAX,
                    );

                    a.emit_cvttss2si_32(XMMOrMemory::XMM(tmp_in), tmp_out);
                    a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }
            Operator::I32TruncSatF32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S32,
                    loc,
                    Location::XMM(tmp_in),
                );
                Self::emit_f32_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    tmp_in,
                    GEF32_LT_I32_MIN,
                    LEF32_GT_I32_MAX,
                    |a, _m| {
                        a.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MIN as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MAX as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|a: &mut Assembler, _m: &mut Machine| {
                        a.emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out));
                    }),
                    |a, _m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i32_trunc_sf32(tmp_in, tmp_out);
                        } else {
                            a.emit_cvttss2si_32(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i64_trunc_sf32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    Self::emit_f32_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        tmp_in,
                        GEF32_LT_I64_MIN,
                        LEF32_GT_I64_MAX,
                    );
                    a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I64TruncSatF32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S32,
                    loc,
                    Location::XMM(tmp_in),
                );
                Self::emit_f32_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    tmp_in,
                    GEF32_LT_I64_MIN,
                    LEF32_GT_I64_MAX,
                    |a, _m| {
                        a.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MIN as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MAX as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|a: &mut Assembler, _m: &mut Machine| {
                        a.emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out));
                    }),
                    |a, _m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i64_trunc_sf32(tmp_in, tmp_out);
                        } else {
                            a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF32U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i64_trunc_uf32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap(); // xmm2

                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    Self::emit_f32_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        tmp_in,
                        GEF32_LT_U64_MIN,
                        LEF32_GT_U64_MAX,
                    );

                    let tmp = self.machine.acquire_temp_gpr().unwrap(); // r15
                    let tmp_x1 = self.machine.acquire_temp_xmm().unwrap(); // xmm1
                    let tmp_x2 = self.machine.acquire_temp_xmm().unwrap(); // xmm3

                    a.emit_mov(
                        Size::S32,
                        Location::Imm32(1593835520u32),
                        Location::GPR(tmp),
                    ); //float 9.22337203E+18
                    a.emit_mov(Size::S32, Location::GPR(tmp), Location::XMM(tmp_x1));
                    a.emit_mov(Size::S32, Location::XMM(tmp_in), Location::XMM(tmp_x2));
                    a.emit_vsubss(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in);
                    a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    a.emit_mov(
                        Size::S64,
                        Location::Imm64(0x8000000000000000u64),
                        Location::GPR(tmp),
                    );
                    a.emit_xor(Size::S64, Location::GPR(tmp_out), Location::GPR(tmp));
                    a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                    a.emit_ucomiss(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                    a.emit_cmovae_gpr_64(tmp, tmp_out);
                    a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_x2);
                    self.machine.release_temp_xmm(tmp_x1);
                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }
            Operator::I64TruncSatF32U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S32,
                    loc,
                    Location::XMM(tmp_in),
                );
                Self::emit_f32_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    tmp_in,
                    GEF32_LT_U64_MIN,
                    LEF32_GT_U64_MAX,
                    |a, _m| {
                        a.emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out));
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S64,
                            Location::Imm64(std::u64::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(_a: &mut Assembler, _m: &mut Machine)>,
                    |a, m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i64_trunc_uf32(tmp_in, tmp_out);
                        } else {
                            let tmp = m.acquire_temp_gpr().unwrap();
                            let tmp_x1 = m.acquire_temp_xmm().unwrap();
                            let tmp_x2 = m.acquire_temp_xmm().unwrap();

                            a.emit_mov(
                                Size::S32,
                                Location::Imm32(1593835520u32),
                                Location::GPR(tmp),
                            ); //float 9.22337203E+18
                            a.emit_mov(Size::S32, Location::GPR(tmp), Location::XMM(tmp_x1));
                            a.emit_mov(Size::S32, Location::XMM(tmp_in), Location::XMM(tmp_x2));
                            a.emit_vsubss(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in);
                            a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                            a.emit_mov(
                                Size::S64,
                                Location::Imm64(0x8000000000000000u64),
                                Location::GPR(tmp),
                            );
                            a.emit_xor(Size::S64, Location::GPR(tmp_out), Location::GPR(tmp));
                            a.emit_cvttss2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                            a.emit_ucomiss(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                            a.emit_cmovae_gpr_64(tmp, tmp_out);

                            m.release_temp_xmm(tmp_x2);
                            m.release_temp_xmm(tmp_x1);
                            m.release_temp_gpr(tmp);
                        }
                    },
                );

                a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I32TruncF64U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i32_trunc_uf64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    Self::emit_f64_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        tmp_in,
                        GEF64_LT_U32_MIN,
                        LEF64_GT_U32_MAX,
                    );

                    a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I32TruncSatF64U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    loc,
                    Location::XMM(tmp_in),
                );
                Self::emit_f64_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    tmp_in,
                    GEF64_LT_U32_MIN,
                    LEF64_GT_U32_MAX,
                    |a, _m| {
                        a.emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out));
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S32,
                            Location::Imm32(std::u32::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(_a: &mut Assembler, _m: &mut Machine)>,
                    |a, _m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i32_trunc_uf64(tmp_in, tmp_out);
                        } else {
                            a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I32TruncF64S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i32_trunc_sf64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                    let real_in = match loc {
                        Location::Imm32(_) | Location::Imm64(_) => {
                            a.emit_mov(Size::S64, loc, Location::GPR(tmp_out));
                            a.emit_mov(Size::S64, Location::GPR(tmp_out), Location::XMM(tmp_in));
                            tmp_in
                        }
                        Location::XMM(x) => x,
                        _ => {
                            a.emit_mov(Size::S64, loc, Location::XMM(tmp_in));
                            tmp_in
                        }
                    };

                    Self::emit_f64_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        real_in,
                        GEF64_LT_I32_MIN,
                        LEF64_GT_I32_MAX,
                    );

                    a.emit_cvttsd2si_32(XMMOrMemory::XMM(real_in), tmp_out);
                    a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I32TruncSatF64S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                let real_in = match loc {
                    Location::Imm32(_) | Location::Imm64(_) => {
                        a.emit_mov(Size::S64, loc, Location::GPR(tmp_out));
                        a.emit_mov(Size::S64, Location::GPR(tmp_out), Location::XMM(tmp_in));
                        tmp_in
                    }
                    Location::XMM(x) => x,
                    _ => {
                        a.emit_mov(Size::S64, loc, Location::XMM(tmp_in));
                        tmp_in
                    }
                };

                Self::emit_f64_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    real_in,
                    GEF64_LT_I32_MIN,
                    LEF64_GT_I32_MAX,
                    |a, _m| {
                        a.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MIN as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S32,
                            Location::Imm32(std::i32::MAX as u32),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|a: &mut Assembler, _m: &mut Machine| {
                        a.emit_mov(Size::S32, Location::Imm32(0), Location::GPR(tmp_out));
                    }),
                    |a, _m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i32_trunc_sf64(tmp_in, tmp_out);
                        } else {
                            a.emit_cvttsd2si_32(XMMOrMemory::XMM(real_in), tmp_out);
                        }
                    },
                );

                a.emit_mov(Size::S32, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF64S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i64_trunc_sf64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    Self::emit_f64_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        tmp_in,
                        GEF64_LT_I64_MIN,
                        LEF64_GT_I64_MAX,
                    );

                    a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I64TruncSatF64S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    loc,
                    Location::XMM(tmp_in),
                );
                Self::emit_f64_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    tmp_in,
                    GEF64_LT_I64_MIN,
                    LEF64_GT_I64_MAX,
                    |a, _m| {
                        a.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MIN as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S64,
                            Location::Imm64(std::i64::MAX as u64),
                            Location::GPR(tmp_out),
                        );
                    },
                    Some(|a: &mut Assembler, _m: &mut Machine| {
                        a.emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out));
                    }),
                    |a, _m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i64_trunc_sf64(tmp_in, tmp_out);
                        } else {
                            a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                        }
                    },
                );

                a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::I64TruncF64U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_itruncf() {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    a.arch_emit_i64_trunc_uf64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::GPR(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                    let tmp_in = self.machine.acquire_temp_xmm().unwrap(); // xmm2

                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::XMM(tmp_in),
                    );
                    Self::emit_f64_int_conv_check_trap(
                        a,
                        &mut self.machine,
                        tmp_in,
                        GEF64_LT_U64_MIN,
                        LEF64_GT_U64_MAX,
                    );

                    let tmp = self.machine.acquire_temp_gpr().unwrap(); // r15
                    let tmp_x1 = self.machine.acquire_temp_xmm().unwrap(); // xmm1
                    let tmp_x2 = self.machine.acquire_temp_xmm().unwrap(); // xmm3

                    a.emit_mov(
                        Size::S64,
                        Location::Imm64(4890909195324358656u64),
                        Location::GPR(tmp),
                    ); //double 9.2233720368547758E+18
                    a.emit_mov(Size::S64, Location::GPR(tmp), Location::XMM(tmp_x1));
                    a.emit_mov(Size::S64, Location::XMM(tmp_in), Location::XMM(tmp_x2));
                    a.emit_vsubsd(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in);
                    a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                    a.emit_mov(
                        Size::S64,
                        Location::Imm64(0x8000000000000000u64),
                        Location::GPR(tmp),
                    );
                    a.emit_xor(Size::S64, Location::GPR(tmp_out), Location::GPR(tmp));
                    a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                    a.emit_ucomisd(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                    a.emit_cmovae_gpr_64(tmp, tmp_out);
                    a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);

                    self.machine.release_temp_xmm(tmp_x2);
                    self.machine.release_temp_xmm(tmp_x1);
                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_xmm(tmp_in);
                    self.machine.release_temp_gpr(tmp_out);
                }
            }

            Operator::I64TruncSatF64U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let tmp_out = self.machine.acquire_temp_gpr().unwrap();
                let tmp_in = self.machine.acquire_temp_xmm().unwrap();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_mov,
                    Size::S64,
                    loc,
                    Location::XMM(tmp_in),
                );
                Self::emit_f64_int_conv_check_sat(
                    a,
                    &mut self.machine,
                    tmp_in,
                    GEF64_LT_U64_MIN,
                    LEF64_GT_U64_MAX,
                    |a, _m| {
                        a.emit_mov(Size::S64, Location::Imm64(0), Location::GPR(tmp_out));
                    },
                    |a, _m| {
                        a.emit_mov(
                            Size::S64,
                            Location::Imm64(std::u64::MAX),
                            Location::GPR(tmp_out),
                        );
                    },
                    None::<fn(_a: &mut Assembler, _m: &mut Machine)>,
                    |a, m| {
                        if a.arch_has_itruncf() {
                            a.arch_emit_i64_trunc_uf64(tmp_in, tmp_out);
                        } else {
                            let tmp = m.acquire_temp_gpr().unwrap();
                            let tmp_x1 = m.acquire_temp_xmm().unwrap();
                            let tmp_x2 = m.acquire_temp_xmm().unwrap();

                            a.emit_mov(
                                Size::S64,
                                Location::Imm64(4890909195324358656u64),
                                Location::GPR(tmp),
                            ); //double 9.2233720368547758E+18
                            a.emit_mov(Size::S64, Location::GPR(tmp), Location::XMM(tmp_x1));
                            a.emit_mov(Size::S64, Location::XMM(tmp_in), Location::XMM(tmp_x2));
                            a.emit_vsubsd(tmp_in, XMMOrMemory::XMM(tmp_x1), tmp_in);
                            a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_in), tmp_out);
                            a.emit_mov(
                                Size::S64,
                                Location::Imm64(0x8000000000000000u64),
                                Location::GPR(tmp),
                            );
                            a.emit_xor(Size::S64, Location::GPR(tmp_out), Location::GPR(tmp));
                            a.emit_cvttsd2si_64(XMMOrMemory::XMM(tmp_x2), tmp_out);
                            a.emit_ucomisd(XMMOrMemory::XMM(tmp_x1), tmp_x2);
                            a.emit_cmovae_gpr_64(tmp, tmp_out);

                            m.release_temp_xmm(tmp_x2);
                            m.release_temp_xmm(tmp_x1);
                            m.release_temp_gpr(tmp);
                        }
                    },
                );

                a.emit_mov(Size::S64, Location::GPR(tmp_out), ret);
                self.machine.release_temp_xmm(tmp_in);
                self.machine.release_temp_gpr(tmp_out);
            }

            Operator::F32ConvertI32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f32_convert_si32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    a.emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    a.emit_vcvtsi2ss_32(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_mov(Size::S32, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }
            Operator::F32ConvertI32U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f32_convert_ui32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    a.emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    a.emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_mov(Size::S32, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }
            Operator::F32ConvertI64S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f32_convert_si64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    a.emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    a.emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_mov(Size::S32, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }
            Operator::F32ConvertI64U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f32_convert_ui64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    let tmp = self.machine.acquire_temp_gpr().unwrap();

                    let do_convert = a.get_label();
                    let end_convert = a.get_label();

                    a.emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    a.emit_test_gpr_64(tmp_in);
                    a.emit_jmp(Condition::Signed, do_convert);
                    a.emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_jmp(Condition::None, end_convert);
                    a.emit_label(do_convert);
                    a.emit_mov(Size::S64, Location::GPR(tmp_in), Location::GPR(tmp));
                    a.emit_and(Size::S64, Location::Imm32(1), Location::GPR(tmp));
                    a.emit_shr(Size::S64, Location::Imm8(1), Location::GPR(tmp_in));
                    a.emit_or(Size::S64, Location::GPR(tmp), Location::GPR(tmp_in));
                    a.emit_vcvtsi2ss_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_vaddss(tmp_out, XMMOrMemory::XMM(tmp_out), tmp_out);
                    a.emit_label(end_convert);
                    a.emit_mov(Size::S32, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }

            Operator::F64ConvertI32S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f64_convert_si32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    a.emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    a.emit_vcvtsi2sd_32(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_mov(Size::S64, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }
            Operator::F64ConvertI32U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S32,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f64_convert_ui32(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    a.emit_mov(Size::S32, loc, Location::GPR(tmp_in));
                    a.emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_mov(Size::S64, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }
            Operator::F64ConvertI64S => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f64_convert_si64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();

                    a.emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    a.emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_mov(Size::S64, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }
            Operator::F64ConvertI64U => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                if a.arch_has_fconverti() {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::GPR(tmp_in),
                    );
                    a.arch_emit_f64_convert_ui64(tmp_in, tmp_out);
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        Location::XMM(tmp_out),
                        ret,
                    );
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                } else {
                    let tmp_out = self.machine.acquire_temp_xmm().unwrap();
                    let tmp_in = self.machine.acquire_temp_gpr().unwrap();
                    let tmp = self.machine.acquire_temp_gpr().unwrap();

                    let do_convert = a.get_label();
                    let end_convert = a.get_label();

                    a.emit_mov(Size::S64, loc, Location::GPR(tmp_in));
                    a.emit_test_gpr_64(tmp_in);
                    a.emit_jmp(Condition::Signed, do_convert);
                    a.emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_jmp(Condition::None, end_convert);
                    a.emit_label(do_convert);
                    a.emit_mov(Size::S64, Location::GPR(tmp_in), Location::GPR(tmp));
                    a.emit_and(Size::S64, Location::Imm32(1), Location::GPR(tmp));
                    a.emit_shr(Size::S64, Location::Imm8(1), Location::GPR(tmp_in));
                    a.emit_or(Size::S64, Location::GPR(tmp), Location::GPR(tmp_in));
                    a.emit_vcvtsi2sd_64(tmp_out, GPROrMemory::GPR(tmp_in), tmp_out);
                    a.emit_vaddsd(tmp_out, XMMOrMemory::XMM(tmp_out), tmp_out);
                    a.emit_label(end_convert);
                    a.emit_mov(Size::S64, Location::XMM(tmp_out), ret);

                    self.machine.release_temp_gpr(tmp);
                    self.machine.release_temp_gpr(tmp_in);
                    self.machine.release_temp_xmm(tmp_out);
                }
            }

            Operator::Call { function_index } => {
                let function_index = function_index as usize;
                let label = self
                    .function_labels
                    .as_mut()
                    .unwrap()
                    .entry(function_index)
                    .or_insert_with(|| (a.get_label(), None))
                    .0;
                let sig_index = *self
                    .function_signatures
                    .get(FuncIndex::new(function_index))
                    .unwrap();
                let sig = self.signatures.get(sig_index).unwrap();
                let param_types: SmallVec<[WpType; 8]> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.returns().iter().cloned().map(type_to_wp_type).collect();

                let params: SmallVec<[_; 8]> = self
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();
                self.machine.release_locations_only_regs(&params);

                self.machine.release_locations_only_osr_state(params.len());

                Self::emit_call_sysv_label(
                    a,
                    &mut self.machine,
                    label,
                    params.iter().map(|x| *x),
                    Some((&mut self.fsm, &mut self.control_stack)),
                )?;

                self.machine.release_locations_only_stack(a, &params);

                if return_types.len() > 0 {
                    let ret = self.machine.acquire_locations(
                        a,
                        &[(
                            return_types[0],
                            MachineValue::WasmStack(self.value_stack.len()),
                        )],
                        false,
                    )[0];
                    self.value_stack.push(ret);
                    a.emit_mov(Size::S64, Location::GPR(GPR::RAX), ret);
                }
            }
            Operator::CallIndirect { index, table_index } => {
                if table_index != 0 {
                    return Err(CodegenError {
                        message: format!("CallIndirect: table_index is not 0"),
                    });
                }
                let sig = self.signatures.get(SigIndex::new(index as usize)).unwrap();
                let param_types: SmallVec<[WpType; 8]> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: SmallVec<[WpType; 1]> =
                    sig.returns().iter().cloned().map(type_to_wp_type).collect();

                let func_index =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                let params: SmallVec<[_; 8]> = self
                    .value_stack
                    .drain(self.value_stack.len() - param_types.len()..)
                    .collect();
                self.machine.release_locations_only_regs(&params);

                let table_base = self.machine.acquire_temp_gpr().unwrap();
                let table_count = self.machine.acquire_temp_gpr().unwrap();
                let sigidx = self.machine.acquire_temp_gpr().unwrap();

                a.emit_mov(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        match TableIndex::new(0).local_or_import(module_info) {
                            LocalOrImport::Local(_) => vm::Ctx::offset_tables(),
                            LocalOrImport::Import(_) => vm::Ctx::offset_imported_tables(),
                        } as i32,
                    ),
                    Location::GPR(table_base),
                );
                a.emit_mov(
                    Size::S64,
                    Location::Memory(table_base, 0),
                    Location::GPR(table_base),
                );
                a.emit_mov(
                    Size::S32,
                    Location::Memory(table_base, LocalTable::offset_count() as i32),
                    Location::GPR(table_count),
                );
                a.emit_mov(
                    Size::S64,
                    Location::Memory(table_base, LocalTable::offset_base() as i32),
                    Location::GPR(table_base),
                );
                a.emit_cmp(Size::S32, func_index, Location::GPR(table_count));
                a.emit_conditional_trap(Condition::BelowEqual);
                a.emit_mov(Size::S64, func_index, Location::GPR(table_count));
                a.emit_imul_imm32_gpr64(vm::Anyfunc::size() as u32, table_count);
                a.emit_add(
                    Size::S64,
                    Location::GPR(table_base),
                    Location::GPR(table_count),
                );
                a.emit_mov(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        vm::Ctx::offset_signatures() as i32,
                    ),
                    Location::GPR(sigidx),
                );
                a.emit_mov(
                    Size::S32,
                    Location::Memory(sigidx, (index * 4) as i32),
                    Location::GPR(sigidx),
                );
                a.emit_cmp(
                    Size::S32,
                    Location::GPR(sigidx),
                    Location::Memory(table_count, (vm::Anyfunc::offset_sig_id() as usize) as i32),
                );
                a.emit_conditional_trap(Condition::NotEqual);

                self.machine.release_temp_gpr(sigidx);
                self.machine.release_temp_gpr(table_count);
                self.machine.release_temp_gpr(table_base);

                if table_count != GPR::RAX {
                    a.emit_mov(
                        Size::S64,
                        Location::GPR(table_count),
                        Location::GPR(GPR::RAX),
                    );
                }

                self.machine.release_locations_only_osr_state(params.len());

                Self::emit_call_sysv(
                    a,
                    &mut self.machine,
                    |a| {
                        if a.arch_requires_indirect_call_trampoline() {
                            a.arch_emit_indirect_call_with_trampoline(Location::Memory(
                                GPR::RAX,
                                (vm::Anyfunc::offset_func() as usize) as i32,
                            ));
                        } else {
                            a.emit_call_location(Location::Memory(
                                GPR::RAX,
                                (vm::Anyfunc::offset_func() as usize) as i32,
                            ));
                        }
                    },
                    params.iter().map(|x| *x),
                    Some((&mut self.fsm, &mut self.control_stack)),
                )?;

                self.machine.release_locations_only_stack(a, &params);

                if return_types.len() > 0 {
                    let ret = self.machine.acquire_locations(
                        a,
                        &[(
                            return_types[0],
                            MachineValue::WasmStack(self.value_stack.len()),
                        )],
                        false,
                    )[0];
                    self.value_stack.push(ret);
                    a.emit_mov(Size::S64, Location::GPR(GPR::RAX), ret);
                }
            }
            Operator::If { ty } => {
                let label_end = a.get_label();
                let label_else = a.get_label();

                let cond =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                let frame = ControlFrame {
                    label: label_end,
                    loop_like: false,
                    if_else: IfElseState::If(label_else),
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: format!("If: multi-value returns not yet implemented"),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id: Self::get_state_diff(
                        &self.machine,
                        &mut self.fsm,
                        &mut self.control_stack,
                    ),
                };
                self.control_stack.push(frame);
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S32,
                    Location::Imm32(0),
                    cond,
                );
                a.emit_jmp(Condition::Equal, label_else);
            }
            Operator::Else => {
                let mut frame = self.control_stack.last_mut().unwrap();

                if !was_unreachable && frame.returns.len() > 0 {
                    let loc = *self.value_stack.last().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::GPR(GPR::RAX),
                    );
                }

                let released: &[Location] = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations(a, released);
                self.value_stack.truncate(frame.value_stack_depth);

                match frame.if_else {
                    IfElseState::If(label) => {
                        a.emit_jmp(Condition::None, frame.label);
                        a.emit_label(label);
                        frame.if_else = IfElseState::Else;
                    }
                    _ => {
                        return Err(CodegenError {
                            message: format!("Else: frame.if_else unreachable code"),
                        })
                    }
                }
            }
            Operator::Select => {
                let cond =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let v_b =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let v_a =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let end_label = a.get_label();
                let zero_label = a.get_label();

                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S32,
                    Location::Imm32(0),
                    cond,
                );
                a.emit_jmp(Condition::Equal, zero_label);
                if v_a != ret {
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        v_a,
                        ret,
                    );
                }
                a.emit_jmp(Condition::None, end_label);
                a.emit_label(zero_label);
                if v_b != ret {
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        v_b,
                        ret,
                    );
                }
                a.emit_label(end_label);
            }
            Operator::Block { ty } => {
                let frame = ControlFrame {
                    label: a.get_label(),
                    loop_like: false,
                    if_else: IfElseState::None,
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: format!("Block: multi-value returns not yet implemented"),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id: Self::get_state_diff(
                        &self.machine,
                        &mut self.fsm,
                        &mut self.control_stack,
                    ),
                };
                self.control_stack.push(frame);
            }
            Operator::Loop { ty } => {
                let label = a.get_label();
                let state_diff_id =
                    Self::get_state_diff(&self.machine, &mut self.fsm, &mut self.control_stack);
                let activate_offset = a.get_offset().0;

                self.control_stack.push(ControlFrame {
                    label: label,
                    loop_like: true,
                    if_else: IfElseState::None,
                    returns: match ty {
                        WpTypeOrFuncType::Type(WpType::EmptyBlockType) => smallvec![],
                        WpTypeOrFuncType::Type(inner_ty) => smallvec![inner_ty],
                        _ => {
                            return Err(CodegenError {
                                message: format!("Loop: multi-value returns not yet implemented"),
                            })
                        }
                    },
                    value_stack_depth: self.value_stack.len(),
                    state: self.machine.state.clone(),
                    state_diff_id,
                });
                a.emit_label(label);

                // Check interrupt signal without branching
                a.emit_mov(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        vm::Ctx::offset_interrupt_signal_mem() as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );
                self.fsm.loop_offsets.insert(
                    a.get_offset().0,
                    OffsetInfo {
                        end_offset: a.get_offset().0 + 1,
                        activate_offset,
                        diff_id: state_diff_id,
                    },
                );
                self.fsm.wasm_offset_to_target_offset.insert(
                    self.machine.state.wasm_inst_offset,
                    SuspendOffset::Loop(a.get_offset().0),
                );
                a.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RAX, 0),
                    Location::GPR(GPR::RAX),
                );
            }
            Operator::Nop => {}
            Operator::MemorySize { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                a.emit_mov(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        vm::Ctx::offset_intrinsics() as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );
                a.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RAX, vm::Intrinsics::offset_memory_size() as i32),
                    Location::GPR(GPR::RAX),
                );
                Self::emit_call_sysv(
                    a,
                    &mut self.machine,
                    |a| {
                        let label = a.get_label();
                        let after = a.get_label();
                        a.emit_jmp(Condition::None, after);
                        a.emit_label(label);
                        a.emit_host_redirection(GPR::RAX);
                        a.emit_label(after);
                        a.emit_call_label(label);
                    },
                    iter::once(Location::Imm32(memory_index.index() as u32)),
                    None,
                )?;
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                a.emit_mov(Size::S64, Location::GPR(GPR::RAX), ret);
            }
            Operator::MemoryGrow { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                let param_pages = self.value_stack.pop().unwrap();

                self.machine.release_locations_only_regs(&[param_pages]);

                a.emit_mov(
                    Size::S64,
                    Location::Memory(
                        Machine::get_vmctx_reg(),
                        vm::Ctx::offset_intrinsics() as i32,
                    ),
                    Location::GPR(GPR::RAX),
                );
                a.emit_mov(
                    Size::S64,
                    Location::Memory(GPR::RAX, vm::Intrinsics::offset_memory_grow() as i32),
                    Location::GPR(GPR::RAX),
                );

                self.machine.release_locations_only_osr_state(1);

                Self::emit_call_sysv(
                    a,
                    &mut self.machine,
                    |a| {
                        let label = a.get_label();
                        let after = a.get_label();
                        a.emit_jmp(Condition::None, after);
                        a.emit_label(label);
                        a.emit_host_redirection(GPR::RAX);
                        a.emit_label(after);
                        a.emit_call_label(label);
                    },
                    iter::once(Location::Imm32(memory_index.index() as u32))
                        .chain(iter::once(param_pages)),
                    None,
                )?;

                self.machine.release_locations_only_stack(a, &[param_pages]);

                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);
                a.emit_mov(Size::S64, Location::GPR(GPR::RAX), ret);
            }
            Operator::I32Load { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::F32Load { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I32Load8U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S8,
                            Location::Memory(addr, 0),
                            Size::S32,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I32Load8S { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movsx,
                            Size::S8,
                            Location::Memory(addr, 0),
                            Size::S32,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I32Load16U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S16,
                            Location::Memory(addr, 0),
                            Size::S32,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I32Load16S { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movsx,
                            Size::S16,
                            Location::Memory(addr, 0),
                            Size::S32,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I32Store { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::F32Store { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I32Store8 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S8,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I32Store16 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S16,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64Load { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    8,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S64,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::F64Load { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::F64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    8,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S64,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64Load8U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S8,
                            Location::Memory(addr, 0),
                            Size::S64,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I64Load8S { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movsx,
                            Size::S8,
                            Location::Memory(addr, 0),
                            Size::S64,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I64Load16U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S16,
                            Location::Memory(addr, 0),
                            Size::S64,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I64Load16S { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movsx,
                            Size::S16,
                            Location::Memory(addr, 0),
                            Size::S64,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I64Load32U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    4,
                    |a, m, addr| {
                        match ret {
                            Location::GPR(_) => {}
                            Location::Memory(base, offset) => {
                                a.emit_mov(
                                    Size::S32,
                                    Location::Imm32(0),
                                    Location::Memory(base, offset + 4),
                                ); // clear upper bits
                            }
                            _ => {
                                return Err(CodegenError {
                                    message: format!("I64Load32U ret: unreachable code"),
                                })
                            }
                        }
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64Load32S { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    false,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movsx,
                            Size::S32,
                            Location::Memory(addr, 0),
                            Size::S64,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I64Store { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    8,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S64,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::F64Store { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    8,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S64,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64Store8 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S8,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64Store16 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S16,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64Store32 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    false,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::Unreachable => {
                Self::mark_trappable(a, &self.machine, &mut self.fsm, &mut self.control_stack);
                a.emit_ud2();
                self.unreachable_depth = 1;
            }
            Operator::Return => {
                let frame = &self.control_stack[0];
                if frame.returns.len() > 0 {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: format!("Return: incorrect frame.returns"),
                        });
                    }
                    let loc = *self.value_stack.last().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::GPR(GPR::RAX),
                    );
                }
                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(a, released);
                a.emit_jmp(Condition::None, frame.label);
                self.unreachable_depth = 1;
            }
            Operator::Br { relative_depth } => {
                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && frame.returns.len() > 0 {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: format!("Br: incorrect frame.returns"),
                        });
                    }
                    let loc = *self.value_stack.last().unwrap();
                    a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                }
                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(a, released);
                a.emit_jmp(Condition::None, frame.label);
                self.unreachable_depth = 1;
            }
            Operator::BrIf { relative_depth } => {
                let after = a.get_label();
                let cond =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S32,
                    Location::Imm32(0),
                    cond,
                );
                a.emit_jmp(Condition::Equal, after);

                let frame =
                    &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                if !frame.loop_like && frame.returns.len() > 0 {
                    if frame.returns.len() != 1 {
                        return Err(CodegenError {
                            message: format!("BrIf: incorrect frame.returns"),
                        });
                    }
                    let loc = *self.value_stack.last().unwrap();
                    a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                }
                let released = &self.value_stack[frame.value_stack_depth..];
                self.machine.release_locations_keep_state(a, released);
                a.emit_jmp(Condition::None, frame.label);

                a.emit_label(after);
            }
            Operator::BrTable { ref table } => {
                let (targets, default_target) = table.read_table().map_err(|e| CodegenError {
                    message: format!("BrTable read_table: {:?}", e),
                })?;
                let cond =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let table_label = a.get_label();
                let mut table: Vec<DynamicLabel> = vec![];
                let default_br = a.get_label();
                Self::emit_relaxed_binop(
                    a,
                    &mut self.machine,
                    Assembler::emit_cmp,
                    Size::S32,
                    Location::Imm32(targets.len() as u32),
                    cond,
                );
                a.emit_jmp(Condition::AboveEqual, default_br);

                a.emit_lea_label(table_label, Location::GPR(GPR::RCX));
                a.emit_mov(Size::S32, cond, Location::GPR(GPR::RDX));

                let instr_size = a.get_jmp_instr_size();
                a.emit_imul_imm32_gpr64(instr_size as _, GPR::RDX);
                a.emit_add(Size::S64, Location::GPR(GPR::RCX), Location::GPR(GPR::RDX));
                a.emit_jmp_location(Location::GPR(GPR::RDX));

                for target in targets.iter() {
                    let label = a.get_label();
                    a.emit_label(label);
                    table.push(label);
                    let frame =
                        &self.control_stack[self.control_stack.len() - 1 - (*target as usize)];
                    if !frame.loop_like && frame.returns.len() > 0 {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: format!(
                                    "BrTable: incorrect frame.returns for {:?}",
                                    target
                                ),
                            });
                        }
                        let loc = *self.value_stack.last().unwrap();
                        a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    }
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations_keep_state(a, released);
                    a.emit_jmp(Condition::None, frame.label);
                }
                a.emit_label(default_br);

                {
                    let frame = &self.control_stack
                        [self.control_stack.len() - 1 - (default_target as usize)];
                    if !frame.loop_like && frame.returns.len() > 0 {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: format!("BrTable: incorrect frame.returns"),
                            });
                        }
                        let loc = *self.value_stack.last().unwrap();
                        a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    }
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations_keep_state(a, released);
                    a.emit_jmp(Condition::None, frame.label);
                }

                a.emit_label(table_label);
                for x in table {
                    a.emit_jmp(Condition::None, x);
                }
                self.unreachable_depth = 1;
            }
            Operator::Drop => {
                get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
            }
            Operator::End => {
                let frame = self.control_stack.pop().unwrap();

                if !was_unreachable && frame.returns.len() > 0 {
                    let loc = *self.value_stack.last().unwrap();
                    Self::emit_relaxed_binop(
                        a,
                        &mut self.machine,
                        Assembler::emit_mov,
                        Size::S64,
                        loc,
                        Location::GPR(GPR::RAX),
                    );
                }

                if self.control_stack.len() == 0 {
                    a.emit_label(frame.label);
                    self.machine.finalize_locals(a, &self.locals);
                    a.emit_mov(Size::S64, Location::GPR(GPR::RBP), Location::GPR(GPR::RSP));
                    a.emit_pop(Size::S64, Location::GPR(GPR::RBP));
                    a.emit_ret();
                } else {
                    let released = &self.value_stack[frame.value_stack_depth..];
                    self.machine.release_locations(a, released);
                    self.value_stack.truncate(frame.value_stack_depth);

                    if !frame.loop_like {
                        a.emit_label(frame.label);
                    }

                    if let IfElseState::If(label) = frame.if_else {
                        a.emit_label(label);
                    }

                    if frame.returns.len() > 0 {
                        if frame.returns.len() != 1 {
                            return Err(CodegenError {
                                message: format!("End: incorrect frame.returns"),
                            });
                        }
                        let loc = self.machine.acquire_locations(
                            a,
                            &[(
                                frame.returns[0],
                                MachineValue::WasmStack(self.value_stack.len()),
                            )],
                            false,
                        )[0];
                        a.emit_mov(Size::S64, Location::GPR(GPR::RAX), loc);
                        self.value_stack.push(loc);
                    }
                }
            }
            Operator::AtomicFence { flags: _ } => {
                // Fence is a nop.
                //
                // Fence was added to preserve information about fences from
                // source languages. If in the future Wasm extends the memory
                // model, and if we hadn't recorded what fences used to be there,
                // it would lead to data races that weren't present in the
                // original source language.
            }
            Operator::I32AtomicLoad { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I32AtomicLoad8U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S8,
                            Location::Memory(addr, 0),
                            Size::S32,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I32AtomicLoad16U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S16,
                            Location::Memory(addr, 0),
                            Size::S32,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I32AtomicStore { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    true,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_xchg,
                            Size::S32,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I32AtomicStore8 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    true,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_xchg,
                            Size::S8,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I32AtomicStore16 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    true,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_xchg,
                            Size::S16,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicLoad { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    8,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S64,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicLoad8U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S8,
                            Location::Memory(addr, 0),
                            Size::S64,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicLoad16U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_zx_sx(
                            a,
                            m,
                            Assembler::emit_movzx,
                            Size::S16,
                            Location::Memory(addr, 0),
                            Size::S64,
                            ret,
                        )?;
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicLoad32U { ref memarg } => {
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, m, addr| {
                        match ret {
                            Location::GPR(_) => {}
                            Location::Memory(base, offset) => {
                                a.emit_mov(
                                    Size::S32,
                                    Location::Imm32(0),
                                    Location::Memory(base, offset + 4),
                                ); // clear upper bits
                            }
                            _ => {
                                return Err(CodegenError {
                                    message: format!("I64AtomicLoad32U ret: unreachable code"),
                                })
                            }
                        }
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_mov,
                            Size::S32,
                            Location::Memory(addr, 0),
                            ret,
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicStore { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    true,
                    8,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_xchg,
                            Size::S64,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicStore8 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    true,
                    1,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_xchg,
                            Size::S8,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicStore16 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    true,
                    2,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_xchg,
                            Size::S16,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I64AtomicStore32 { ref memarg } => {
                let target_value =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target_addr =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target_addr,
                    memarg,
                    true,
                    4,
                    |a, m, addr| {
                        Self::emit_relaxed_binop(
                            a,
                            m,
                            Assembler::emit_xchg,
                            Size::S32,
                            target_value,
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
            }
            Operator::I32AtomicRmwAdd { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmwAdd { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S64, loc, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    8,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S64,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmw8AddU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S8, loc, Size::S32, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_xadd(Size::S8, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmw16AddU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S16, loc, Size::S32, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw8AddU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S8, loc, Size::S64, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_xadd(Size::S8, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw16AddU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S16, loc, Size::S64, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw32AddU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmwSub { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(value));
                a.emit_neg(Size::S32, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmwSub { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S64, loc, Location::GPR(value));
                a.emit_neg(Size::S64, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    8,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S64,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmw8SubU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S8, loc, Size::S32, Location::GPR(value));
                a.emit_neg(Size::S8, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_xadd(Size::S8, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmw16SubU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S16, loc, Size::S32, Location::GPR(value));
                a.emit_neg(Size::S16, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw8SubU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S8, loc, Size::S64, Location::GPR(value));
                a.emit_neg(Size::S8, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_xadd(Size::S8, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw16SubU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S16, loc, Size::S64, Location::GPR(value));
                a.emit_neg(Size::S16, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw32SubU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(value));
                a.emit_neg(Size::S32, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, _m, addr| {
                        a.emit_lock_xadd(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmwAnd { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    4,
                    Size::S32,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_and(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmwAnd { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    8,
                    Size::S64,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_and(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmw8AndU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_and(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmw16AndU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_and(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw8AndU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_and(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw16AndU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_and(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw32AndU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S32,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_and(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmwOr { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    4,
                    Size::S32,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_or(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmwOr { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    8,
                    Size::S64,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_or(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmw8OrU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_or(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmw16OrU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_or(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw8OrU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_or(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw16OrU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_or(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw32OrU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S32,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_or(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmwXor { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    4,
                    Size::S32,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_xor(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmwXor { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    8,
                    Size::S64,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_xor(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmw8XorU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_xor(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmw16XorU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S32,
                    |a, _m, src, dst| {
                        a.emit_xor(Size::S32, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw8XorU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S8,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_xor(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw16XorU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S16,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_xor(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I64AtomicRmw32XorU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                Self::emit_compare_and_swap(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    loc,
                    target,
                    ret,
                    memarg,
                    1,
                    Size::S32,
                    Size::S64,
                    |a, _m, src, dst| {
                        a.emit_xor(Size::S64, Location::GPR(src), Location::GPR(dst));
                    },
                )?;
            }
            Operator::I32AtomicRmwXchg { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, _m, addr| {
                        a.emit_xchg(Size::S32, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmwXchg { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S64, loc, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    8,
                    |a, _m, addr| {
                        a.emit_xchg(Size::S64, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmw8XchgU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S8, loc, Size::S32, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_xchg(Size::S8, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmw16XchgU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S16, loc, Size::S32, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, _m, addr| {
                        a.emit_xchg(Size::S16, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S32, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw8XchgU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S8, loc, Size::S64, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_xchg(Size::S8, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw16XchgU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_movzx(Size::S16, loc, Size::S64, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    2,
                    |a, _m, addr| {
                        a.emit_xchg(Size::S16, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I64AtomicRmw32XchgU { ref memarg } => {
                let loc =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let value = self.machine.acquire_temp_gpr().unwrap();
                a.emit_mov(Size::S32, loc, Location::GPR(value));
                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, _m, addr| {
                        a.emit_xchg(Size::S32, Location::GPR(value), Location::Memory(addr, 0));
                        Ok(())
                    },
                )?;
                a.emit_mov(Size::S64, Location::GPR(value), ret);
                self.machine.release_temp_gpr(value);
            }
            Operator::I32AtomicRmwCmpxchg { ref memarg } => {
                let new =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let cmp =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
                let value = if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                };
                a.emit_push(Size::S64, Location::GPR(value));
                a.emit_mov(Size::S32, cmp, Location::GPR(compare));
                a.emit_mov(Size::S32, new, Location::GPR(value));

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    4,
                    |a, _m, addr| {
                        a.emit_lock_cmpxchg(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        a.emit_mov(Size::S32, Location::GPR(compare), ret);
                        Ok(())
                    },
                )?;
                a.emit_pop(Size::S64, Location::GPR(value));
                self.machine.release_temp_gpr(compare);
            }
            Operator::I64AtomicRmwCmpxchg { ref memarg } => {
                let new =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let cmp =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
                let value = if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                };
                a.emit_push(Size::S64, Location::GPR(value));
                a.emit_mov(Size::S64, cmp, Location::GPR(compare));
                a.emit_mov(Size::S64, new, Location::GPR(value));

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    8,
                    |a, _m, addr| {
                        a.emit_lock_cmpxchg(
                            Size::S64,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        a.emit_mov(Size::S64, Location::GPR(compare), ret);
                        Ok(())
                    },
                )?;
                a.emit_pop(Size::S64, Location::GPR(value));
                self.machine.release_temp_gpr(compare);
            }
            Operator::I32AtomicRmw8CmpxchgU { ref memarg } => {
                let new =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let cmp =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
                let value = if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                };
                a.emit_push(Size::S64, Location::GPR(value));
                a.emit_mov(Size::S32, cmp, Location::GPR(compare));
                a.emit_mov(Size::S32, new, Location::GPR(value));

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_cmpxchg(
                            Size::S8,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        a.emit_movzx(Size::S8, Location::GPR(compare), Size::S32, ret);
                        Ok(())
                    },
                )?;
                a.emit_pop(Size::S64, Location::GPR(value));
                self.machine.release_temp_gpr(compare);
            }
            Operator::I32AtomicRmw16CmpxchgU { ref memarg } => {
                let new =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let cmp =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I32, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
                let value = if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                };
                a.emit_push(Size::S64, Location::GPR(value));
                a.emit_mov(Size::S32, cmp, Location::GPR(compare));
                a.emit_mov(Size::S32, new, Location::GPR(value));

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_cmpxchg(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        a.emit_movzx(Size::S16, Location::GPR(compare), Size::S32, ret);
                        Ok(())
                    },
                )?;
                a.emit_pop(Size::S64, Location::GPR(value));
                self.machine.release_temp_gpr(compare);
            }
            Operator::I64AtomicRmw8CmpxchgU { ref memarg } => {
                let new =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let cmp =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
                let value = if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                };
                a.emit_push(Size::S64, Location::GPR(value));
                a.emit_mov(Size::S64, cmp, Location::GPR(compare));
                a.emit_mov(Size::S64, new, Location::GPR(value));

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_cmpxchg(
                            Size::S8,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        a.emit_movzx(Size::S8, Location::GPR(compare), Size::S64, ret);
                        Ok(())
                    },
                )?;
                a.emit_pop(Size::S64, Location::GPR(value));
                self.machine.release_temp_gpr(compare);
            }
            Operator::I64AtomicRmw16CmpxchgU { ref memarg } => {
                let new =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let cmp =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
                let value = if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                };
                a.emit_push(Size::S64, Location::GPR(value));
                a.emit_mov(Size::S64, cmp, Location::GPR(compare));
                a.emit_mov(Size::S64, new, Location::GPR(value));

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_cmpxchg(
                            Size::S16,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        a.emit_movzx(Size::S16, Location::GPR(compare), Size::S64, ret);
                        Ok(())
                    },
                )?;
                a.emit_pop(Size::S64, Location::GPR(value));
                self.machine.release_temp_gpr(compare);
            }
            Operator::I64AtomicRmw32CmpxchgU { ref memarg } => {
                let new =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let cmp =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let target =
                    get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(
                    a,
                    &[(WpType::I64, MachineValue::WasmStack(self.value_stack.len()))],
                    false,
                )[0];
                self.value_stack.push(ret);

                let compare = self.machine.reserve_unused_temp_gpr(GPR::RAX);
                let value = if cmp == Location::GPR(GPR::R14) {
                    if new == Location::GPR(GPR::R13) {
                        GPR::R12
                    } else {
                        GPR::R13
                    }
                } else {
                    GPR::R14
                };
                a.emit_push(Size::S64, Location::GPR(value));
                a.emit_mov(Size::S64, cmp, Location::GPR(compare));
                a.emit_mov(Size::S64, new, Location::GPR(value));

                Self::emit_memory_op(
                    module_info,
                    &self.config,
                    a,
                    &mut self.machine,
                    target,
                    memarg,
                    true,
                    1,
                    |a, _m, addr| {
                        a.emit_lock_cmpxchg(
                            Size::S32,
                            Location::GPR(value),
                            Location::Memory(addr, 0),
                        );
                        a.emit_mov(Size::S32, Location::GPR(compare), ret);
                        Ok(())
                    },
                )?;
                a.emit_pop(Size::S64, Location::GPR(value));
                self.machine.release_temp_gpr(compare);
            }
            _ => {
                return Err(CodegenError {
                    message: format!("not yet implemented: {:?}", op),
                });
            }
        }

        Ok(())
    }
}

fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
        Type::V128 => WpType::V128,
    }
}

fn get_location_released(a: &mut Assembler, m: &mut Machine, loc: Location) -> Location {
    m.release_locations(a, &[loc]);
    loc
}

fn sort_call_movs(movs: &mut [(Location, GPR)]) {
    for i in 0..movs.len() {
        for j in (i + 1)..movs.len() {
            if let Location::GPR(src_gpr) = movs[j].0 {
                if src_gpr == movs[i].1 {
                    movs.swap(i, j);
                }
            }
        }
    }

    /*
    {
        use std::collections::{HashMap, HashSet, VecDeque};
        let mut mov_map: HashMap<GPR, HashSet<GPR>> = HashMap::new();
        for mov in movs.iter() {
            if let Location::GPR(src_gpr) = mov.0 {
                if src_gpr != mov.1 {
                    mov_map.entry(src_gpr).or_insert_with(|| HashSet::new()).insert(mov.1);
                }
            }
        }

        for (start, _) in mov_map.iter() {
            let mut q: VecDeque<GPR> = VecDeque::new();
            let mut black: HashSet<GPR> = HashSet::new();

            q.push_back(*start);
            black.insert(*start);

            while q.len() > 0 {
                let reg = q.pop_front().unwrap();
                let empty_set = HashSet::new();
                for x in mov_map.get(&reg).unwrap_or(&empty_set).iter() {
                    if black.contains(x) {
                        panic!("cycle detected");
                    }
                    q.push_back(*x);
                    black.insert(*x);
                }
            }
        }
    }
    */
}

// Constants for the bounds of truncation operations. These are the least or
// greatest exact floats in either f32 or f64 representation less-than (for
// least) or greater-than (for greatest) the i32 or i64 or u32 or u64
// min (for least) or max (for greatest), when rounding towards zero.

/// Greatest Exact Float (32 bits) less-than i32::MIN when rounding towards zero.
const GEF32_LT_I32_MIN: f32 = -2147483904.0;
/// Least Exact Float (32 bits) greater-than i32::MAX when rounding towards zero.
const LEF32_GT_I32_MAX: f32 = 2147483648.0;
/// Greatest Exact Float (32 bits) less-than i64::MIN when rounding towards zero.
const GEF32_LT_I64_MIN: f32 = -9223373136366403584.0;
/// Least Exact Float (32 bits) greater-than i64::MAX when rounding towards zero.
const LEF32_GT_I64_MAX: f32 = 9223372036854775808.0;
/// Greatest Exact Float (32 bits) less-than u32::MIN when rounding towards zero.
const GEF32_LT_U32_MIN: f32 = -1.0;
/// Least Exact Float (32 bits) greater-than u32::MAX when rounding towards zero.
const LEF32_GT_U32_MAX: f32 = 4294967296.0;
/// Greatest Exact Float (32 bits) less-than u64::MIN when rounding towards zero.
const GEF32_LT_U64_MIN: f32 = -1.0;
/// Least Exact Float (32 bits) greater-than u64::MAX when rounding towards zero.
const LEF32_GT_U64_MAX: f32 = 18446744073709551616.0;

/// Greatest Exact Float (64 bits) less-than i32::MIN when rounding towards zero.
const GEF64_LT_I32_MIN: f64 = -2147483649.0;
/// Least Exact Float (64 bits) greater-than i32::MAX when rounding towards zero.
const LEF64_GT_I32_MAX: f64 = 2147483648.0;
/// Greatest Exact Float (64 bits) less-than i64::MIN when rounding towards zero.
const GEF64_LT_I64_MIN: f64 = -9223372036854777856.0;
/// Least Exact Float (64 bits) greater-than i64::MAX when rounding towards zero.
const LEF64_GT_I64_MAX: f64 = 9223372036854775808.0;
/// Greatest Exact Float (64 bits) less-than u32::MIN when rounding towards zero.
const GEF64_LT_U32_MIN: f64 = -1.0;
/// Least Exact Float (64 bits) greater-than u32::MAX when rounding towards zero.
const LEF64_GT_U32_MAX: f64 = 4294967296.0;
/// Greatest Exact Float (64 bits) less-than u64::MIN when rounding towards zero.
const GEF64_LT_U64_MIN: f64 = -1.0;
/// Least Exact Float (64 bits) greater-than u64::MAX when rounding towards zero.
const LEF64_GT_U64_MAX: f64 = 18446744073709551616.0;
