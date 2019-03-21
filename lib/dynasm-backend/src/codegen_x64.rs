#![allow(clippy::forget_copy)] // Used by dynasm.

use super::codegen::*;
use super::stack::{
    ControlFrame, ControlStack, IfElseState, ScratchRegister, ValueInfo, ValueLocation, ValueStack,
};
use crate::protect_unix;
use byteorder::{ByteOrder, LittleEndian};
use dynasmrt::{
    x64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, ExecutableBuffer,
};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::{any::Any, collections::HashMap, sync::Arc};
use wasmer_runtime_core::{
    backend::{FuncResolver, ProtectedCaller, Token, UserTrapper},
    error::{RuntimeError, RuntimeResult},
    memory::MemoryType,
    module::{ModuleInfo, ModuleInner},
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, ImportedMemoryIndex, LocalFuncIndex, LocalGlobalIndex,
        LocalMemoryIndex, LocalOrImport, MemoryIndex, SigIndex, Type, Value,
    },
    units::Pages,
    vm::{self, ImportBacking, LocalGlobal, LocalMemory, LocalTable},
};
use wasmparser::{Operator, Type as WpType};

thread_local! {
    static CURRENT_EXECUTION_CONTEXT: RefCell<Vec<*const X64ExecutionContext>> = RefCell::new(Vec::new());
}

lazy_static! {
    static ref CALL_WASM: unsafe extern "C" fn(
        params: *const u8,
        params_len: usize,
        target: *const u8,
        memory_base: *mut u8,
        memory_size_pages: usize,
        vmctx: *mut vm::Ctx
    ) -> i64 = {
        let mut assembler = Assembler::new().unwrap();
        let offset = assembler.offset();
        dynasm!(
            assembler
            ; push rbx
            ; push r12
            ; push r13
            ; push r14
            ; push r15

            ; mov r15, rcx // memory_base

            // Use the upper 16 bits of r15 to store memory size (in pages). This can support memory size up to 4GB.
            // Wasmer currently only runs in usermode so here we assume the upper 17 bits of memory base address are all zero.
            // FIXME: Change this if want to use this backend in kernel mode.
            ; shl r8, 48
            ; or r15, r8

            ; mov r14, r9 // vmctx
            ; lea rax, [>after_call]
            ; push rax
            ; push rbp
            ; mov rbp, rsp
            ; sub rsp, rsi // params_len
            ; mov rcx, 0
            ; mov r8, rsp
            ; _loop:
            ; cmp rsi, 0
            ; je >_loop_end
            ; mov rax, [rdi]
            ; mov [r8], rax
            ; add r8, 8
            ; add rdi, 8
            ; sub rsi, 8
            ; jmp <_loop
            ; _loop_end:
            ; jmp rdx
            ; after_call:
            ; pop r15
            ; pop r14
            ; pop r13
            ; pop r12
            ; pop rbx
            ; ret
        );
        let buf = assembler.finalize().unwrap();
        let ret = unsafe { ::std::mem::transmute(buf.ptr(offset)) };
        ::std::mem::forget(buf);
        ret
    };

    static ref CONSTRUCT_STACK_AND_CALL_NATIVE: unsafe extern "C" fn (stack_top: *mut u8, stack_base: *mut u8, ctx: *mut vm::Ctx, target: *const vm::Func) -> u64 = {
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
        let ret = unsafe { ::std::mem::transmute(buf.ptr(offset)) };
        ::std::mem::forget(buf);
        ret
    };
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum Register {
    RAX,
    RCX,
    RDX,
    RBX,
    RSP,
    RBP,
    RSI,
    RDI,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

impl Register {
    pub fn from_scratch_reg(sr: ScratchRegister) -> Register {
        use self::Register::*;
        match sr.raw_id() {
            0 => RDI,
            1 => RSI,
            2 => RDX,
            3 => RCX,
            4 => R8,
            5 => R9,
            6 => R10,
            7 => R11,
            8 => RBX,
            9 => R12,
            // 10 => R13, // R13 is reserved as temporary register.
            // 11 => R14, // R14 is reserved for vmctx.
            // 12 => R15, // R15 is reserved for memory base pointer.
            _ => unreachable!(),
        }
    }

    pub fn is_used(&self, stack: &ValueStack) -> bool {
        for val in &stack.values {
            match val.location {
                ValueLocation::Register(x) => {
                    if Register::from_scratch_reg(x) == *self {
                        return true;
                    }
                }
                ValueLocation::Stack => break,
            }
        }

        false
    }
}

#[allow(dead_code)]
pub struct NativeTrampolines {
    memory_size_dynamic_local: DynamicLabel,
    memory_size_static_local: DynamicLabel,
    memory_size_shared_local: DynamicLabel,
    memory_size_dynamic_import: DynamicLabel,
    memory_size_static_import: DynamicLabel,
    memory_size_shared_import: DynamicLabel,
    memory_grow_dynamic_local: DynamicLabel,
    memory_grow_static_local: DynamicLabel,
    memory_grow_shared_local: DynamicLabel,
    memory_grow_dynamic_import: DynamicLabel,
    memory_grow_static_import: DynamicLabel,
    memory_grow_shared_import: DynamicLabel,
}

pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
    signatures: Option<Arc<Map<SigIndex, FuncSig>>>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    function_labels: Option<HashMap<usize, (DynamicLabel, Option<AssemblyOffset>)>>,
    assembler: Option<Assembler>,
    native_trampolines: Arc<NativeTrampolines>,
    func_import_count: usize,
}

pub struct X64FunctionCode {
    signatures: Arc<Map<SigIndex, FuncSig>>,
    function_signatures: Arc<Map<FuncIndex, SigIndex>>,
    native_trampolines: Arc<NativeTrampolines>,

    begin_offset: AssemblyOffset,
    assembler: Option<Assembler>,
    function_labels: Option<HashMap<usize, (DynamicLabel, Option<AssemblyOffset>)>>,
    br_table_data: Option<Vec<Vec<usize>>>,
    returns: Vec<WpType>,
    locals: Vec<Local>,
    num_params: usize,
    current_stack_offset: usize,
    value_stack: ValueStack,
    control_stack: Option<ControlStack>,
    unreachable_depth: usize,
}

enum FuncPtrInner {}
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
struct FuncPtr(*const FuncPtrInner);
unsafe impl Send for FuncPtr {}
unsafe impl Sync for FuncPtr {}

pub struct X64ExecutionContext {
    code: ExecutableBuffer,
    functions: Vec<X64FunctionCode>,
    signatures: Arc<Map<SigIndex, FuncSig>>,
    function_signatures: Arc<Map<FuncIndex, SigIndex>>,
    function_pointers: Vec<FuncPtr>,
    _br_table_data: Vec<Vec<usize>>,
    func_import_count: usize,
}

pub struct X64RuntimeResolver {
    _code: ExecutableBuffer,
    local_pointers: Vec<FuncPtr>,
}

impl X64ExecutionContext {
    fn get_runtime_resolver(
        &self,
        module_info: &ModuleInfo,
    ) -> Result<X64RuntimeResolver, CodegenError> {
        let mut assembler = Assembler::new().unwrap();
        let mut offsets: Vec<AssemblyOffset> = vec![];

        for i in self.func_import_count..self.function_pointers.len() {
            offsets.push(assembler.offset());
            X64FunctionCode::emit_managed_call_trampoline(
                &mut assembler,
                module_info,
                self.function_pointers[i],
                self.signatures[self.function_signatures[FuncIndex::new(i)]]
                    .params()
                    .len(),
            )?;
        }

        let code = assembler.finalize().unwrap();
        let local_pointers: Vec<FuncPtr> =
            offsets.iter().map(|x| FuncPtr(code.ptr(*x) as _)).collect();

        Ok(X64RuntimeResolver {
            _code: code,
            local_pointers: local_pointers,
        })
    }
}

impl FuncResolver for X64RuntimeResolver {
    fn get(
        &self,
        _module: &ModuleInner,
        _local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        NonNull::new(self.local_pointers[_local_func_index.index() as usize].0 as *mut vm::Func)
    }
}

impl ProtectedCaller for X64ExecutionContext {
    fn call(
        &self,
        _module: &ModuleInner,
        _func_index: FuncIndex,
        _params: &[Value],
        _import_backing: &ImportBacking,
        _vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<Vec<Value>> {
        let index = _func_index.index() - self.func_import_count;
        let ptr = self.code.ptr(self.functions[index].begin_offset);
        let return_ty = self.functions[index].returns.last().cloned();

        if self.functions[index].num_params != _params.len() {
            return Err(RuntimeError::Trap {
                msg: "param count mismatch".into(),
            });
        }

        let f = &self.functions[index];
        let total_size = f.num_params * 8;

        if f.num_params > 0 && f.locals[f.num_params - 1].stack_offset != total_size {
            panic!("internal error: inconsistent stack layout");
        }

        let mut param_buf: Vec<u8> = vec![0; total_size];
        for i in 0..f.num_params {
            let local = &f.locals[i];
            let buf = &mut param_buf[total_size - local.stack_offset..];
            let size = get_size_of_type(&local.ty).unwrap();

            if is_dword(size) {
                match _params[i] {
                    Value::I32(x) => LittleEndian::write_u32(buf, x as u32),
                    Value::F32(x) => LittleEndian::write_u32(buf, f32::to_bits(x)),
                    _ => {
                        return Err(RuntimeError::Trap {
                            msg: "signature mismatch".into(),
                        });
                    }
                }
            } else {
                match _params[i] {
                    Value::I64(x) => LittleEndian::write_u64(buf, x as u64),
                    Value::F64(x) => LittleEndian::write_u64(buf, f64::to_bits(x)),
                    _ => {
                        return Err(RuntimeError::Trap {
                            msg: "signature mismatch".into(),
                        });
                    }
                }
            }
        }

        let (memory_base, memory_size): (*mut u8, usize) = if _module.info.memories.len() > 0 {
            if _module.info.memories.len() != 1 || _module.info.imported_memories.len() != 0 {
                return Err(RuntimeError::Trap {
                    msg: "only one linear memory is supported".into(),
                });
            }
            unsafe {
                let vmctx = _vmctx as *mut vm::InternalCtx;
                ((**(*vmctx).memories).base, (**(*vmctx).memories).bound)
            }
        } else if _module.info.imported_memories.len() > 0 {
            if _module.info.memories.len() != 0 || _module.info.imported_memories.len() != 1 {
                return Err(RuntimeError::Trap {
                    msg: "only one linear memory is supported".into(),
                });
            }
            unsafe {
                let vmctx = _vmctx as *mut vm::InternalCtx;
                (
                    (**(*vmctx).imported_memories).base,
                    (**(*vmctx).imported_memories).bound,
                )
            }
        } else {
            (::std::ptr::null_mut(), 0)
        };
        //println!("MEMORY = {:?}", memory_base);

        CURRENT_EXECUTION_CONTEXT.with(|x| x.borrow_mut().push(self));

        let ret = unsafe {
            protect_unix::call_protected(|| {
                CALL_WASM(
                    param_buf.as_ptr(),
                    param_buf.len(),
                    ptr,
                    memory_base,
                    memory_size.wrapping_shr(16),
                    _vmctx,
                )
            })
        };

        CURRENT_EXECUTION_CONTEXT.with(|x| x.borrow_mut().pop().unwrap());

        let ret = ret?;

        Ok(if let Some(ty) = return_ty {
            vec![match ty {
                WpType::I32 => Value::I32(ret as i32),
                WpType::I64 => Value::I64(ret),
                WpType::F32 => Value::F32(f32::from_bits(ret as i32 as u32)),
                WpType::F64 => Value::F64(f64::from_bits(ret as u64)),
                _ => unreachable!(),
            }]
        } else {
            vec![]
        })
    }

    fn get_early_trapper(&self) -> Box<dyn UserTrapper> {
        pub struct Trapper;

        impl UserTrapper for Trapper {
            unsafe fn do_early_trap(&self, _data: Box<Any>) -> ! {
                panic!("do_early_trap");
            }
        }

        Box::new(Trapper)
    }
}

#[derive(Copy, Clone, Debug)]
struct Local {
    ty: WpType,
    stack_offset: usize,
}

impl X64ModuleCodeGenerator {
    pub fn new() -> X64ModuleCodeGenerator {
        let mut assembler = Assembler::new().unwrap();
        let nt = NativeTrampolines {
            memory_size_dynamic_local: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_size,
                MemoryKind::DynamicLocal,
                0usize,
            ),
            memory_size_static_local: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_size,
                MemoryKind::StaticLocal,
                0usize,
            ),
            memory_size_shared_local: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_size,
                MemoryKind::SharedLocal,
                0usize,
            ),
            memory_size_dynamic_import: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_size,
                MemoryKind::DynamicImport,
                0usize,
            ),
            memory_size_static_import: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_size,
                MemoryKind::StaticImport,
                0usize,
            ),
            memory_size_shared_import: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_size,
                MemoryKind::SharedImport,
                0usize,
            ),
            memory_grow_dynamic_local: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_grow,
                MemoryKind::DynamicLocal,
                0usize,
            ),
            memory_grow_static_local: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_grow,
                MemoryKind::StaticLocal,
                0usize,
            ),
            memory_grow_shared_local: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_grow,
                MemoryKind::SharedLocal,
                0usize,
            ),
            memory_grow_dynamic_import: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_grow,
                MemoryKind::DynamicImport,
                0usize,
            ),
            memory_grow_static_import: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_grow,
                MemoryKind::StaticImport,
                0usize,
            ),
            memory_grow_shared_import: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                _memory_grow,
                MemoryKind::SharedImport,
                0usize,
            ),
        };

        X64ModuleCodeGenerator {
            functions: vec![],
            signatures: None,
            function_signatures: None,
            function_labels: Some(HashMap::new()),
            assembler: Some(assembler),
            native_trampolines: Arc::new(nt),
            func_import_count: 0,
        }
    }
}

impl ModuleCodeGenerator<X64FunctionCode, X64ExecutionContext, X64RuntimeResolver>
    for X64ModuleCodeGenerator
{
    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn next_function(&mut self) -> Result<&mut X64FunctionCode, CodegenError> {
        let (mut assembler, mut function_labels, br_table_data) = match self.functions.last_mut() {
            Some(x) => (
                x.assembler.take().unwrap(),
                x.function_labels.take().unwrap(),
                x.br_table_data.take().unwrap(),
            ),
            None => (
                self.assembler.take().unwrap(),
                self.function_labels.take().unwrap(),
                vec![],
            ),
        };
        let begin_offset = assembler.offset();
        let begin_label_info = function_labels
            .entry(self.functions.len() + self.func_import_count)
            .or_insert_with(|| (assembler.new_dynamic_label(), None));

        begin_label_info.1 = Some(begin_offset);
        let begin_label = begin_label_info.0;

        dynasm!(
            assembler
            ; => begin_label
            //; int 3
        );
        let code = X64FunctionCode {
            signatures: self.signatures.as_ref().unwrap().clone(),
            function_signatures: self.function_signatures.as_ref().unwrap().clone(),
            native_trampolines: self.native_trampolines.clone(),

            begin_offset: begin_offset,
            assembler: Some(assembler),
            function_labels: Some(function_labels),
            br_table_data: Some(br_table_data),
            returns: vec![],
            locals: vec![],
            num_params: 0,
            current_stack_offset: 0,
            value_stack: ValueStack::new(4), // FIXME: Use of R8 and above registers generates incorrect assembly.
            control_stack: None,
            unreachable_depth: 0,
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(
        mut self,
        module_info: &ModuleInfo,
    ) -> Result<(X64ExecutionContext, X64RuntimeResolver), CodegenError> {
        let (assembler, mut br_table_data) = match self.functions.last_mut() {
            Some(x) => (x.assembler.take().unwrap(), x.br_table_data.take().unwrap()),
            None => {
                return Err(CodegenError {
                    message: "no function",
                });
            }
        };
        let output = assembler.finalize().unwrap();

        for table in &mut br_table_data {
            for entry in table {
                *entry = output.ptr(AssemblyOffset(*entry)) as usize;
            }
        }

        let function_labels = if let Some(x) = self.functions.last() {
            x.function_labels.as_ref().unwrap()
        } else {
            self.function_labels.as_ref().unwrap()
        };
        let mut out_labels: Vec<FuncPtr> = vec![];

        for i in 0..function_labels.len() {
            let (_, offset) = match function_labels.get(&i) {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "label not found",
                    });
                }
            };
            let offset = match offset {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "offset is none",
                    });
                }
            };
            out_labels.push(FuncPtr(output.ptr(*offset) as _));
        }

        let ctx = X64ExecutionContext {
            code: output,
            functions: self.functions,
            _br_table_data: br_table_data,
            func_import_count: self.func_import_count,
            signatures: match self.signatures {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "no signatures",
                    });
                }
            },
            function_pointers: out_labels,
            function_signatures: match self.function_signatures {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "no function signatures",
                    });
                }
            },
        };
        let resolver = ctx.get_runtime_resolver(module_info)?;

        Ok((ctx, resolver))
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
        let labels = match self.function_labels.as_mut() {
            Some(x) => x,
            None => {
                return Err(CodegenError {
                    message: "got function import after code",
                });
            }
        };
        let id = labels.len();

        let offset = self.assembler.as_mut().unwrap().offset();

        let label = X64FunctionCode::emit_native_call_trampoline(
            self.assembler.as_mut().unwrap(),
            invoke_import,
            0,
            id,
        );
        labels.insert(id, (label, Some(offset)));

        self.func_import_count += 1;

        Ok(())
    }
}

impl X64FunctionCode {
    fn gen_rt_pop(assembler: &mut Assembler, info: &ValueInfo) -> Result<(), CodegenError> {
        match info.location {
            ValueLocation::Register(_) => {}
            ValueLocation::Stack => {
                dynasm!(
                    assembler
                    ; add rsp, 8
                );
            }
        }
        Ok(())
    }

    fn emit_reinterpret(
        value_stack: &mut ValueStack,
        in_ty: WpType,
        out_ty: WpType,
    ) -> Result<(), CodegenError> {
        let val = value_stack.pop()?;
        if val.ty != in_ty {
            return Err(CodegenError {
                message: "reinterpret type mismatch",
            });
        }
        value_stack.push(out_ty);
        Ok(())
    }

    /// Emits a unary operator.
    fn emit_unop<F: FnOnce(&mut Assembler, &ValueStack, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
        in_ty: WpType,
        out_ty: WpType,
    ) -> Result<(), CodegenError> {
        let a = value_stack.pop()?;
        if a.ty != in_ty {
            return Err(CodegenError {
                message: "unop(i32) type mismatch",
            });
        }
        value_stack.push(out_ty);

        match a.location {
            ValueLocation::Register(x) => {
                let reg = Register::from_scratch_reg(x);
                f(assembler, value_stack, reg);
            }
            ValueLocation::Stack => {
                dynasm!(
                    assembler
                    ; mov rax, [rsp]
                );
                f(assembler, value_stack, Register::RAX);
                dynasm!(
                    assembler
                    ; mov [rsp], rax
                );
            }
        }

        Ok(())
    }

    fn emit_unop_i32<F: FnOnce(&mut Assembler, &ValueStack, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
    ) -> Result<(), CodegenError> {
        Self::emit_unop(assembler, value_stack, f, WpType::I32, WpType::I32)
    }

    fn emit_unop_i64<F: FnOnce(&mut Assembler, &ValueStack, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
    ) -> Result<(), CodegenError> {
        Self::emit_unop(assembler, value_stack, f, WpType::I64, WpType::I64)
    }

    /// Emits a binary operator.
    ///
    /// Guarantees that the first Register parameter to callback `f` will never be `Register::RAX`.
    fn emit_binop<F: FnOnce(&mut Assembler, &ValueStack, Register, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
        in_ty: WpType,
        out_ty: WpType,
    ) -> Result<(), CodegenError> {
        let (a, b) = value_stack.pop2()?;
        if a.ty != in_ty || b.ty != in_ty {
            return Err(CodegenError {
                message: "binop(i32) type mismatch",
            });
        }
        value_stack.push(out_ty);

        if a.location.is_register() && b.location.is_register() {
            // output is in a_reg.
            f(
                assembler,
                value_stack,
                Register::from_scratch_reg(a.location.get_register()?),
                Register::from_scratch_reg(b.location.get_register()?),
            );
        } else if a.location.is_register() {
            dynasm!(
                assembler
                ; pop rax
            );
            f(
                assembler,
                value_stack,
                Register::from_scratch_reg(a.location.get_register()?),
                Register::RAX,
            );
        } else if b.location.is_register() {
            unreachable!();
        } else {
            dynasm!(
                assembler
                ; push rcx
                ; mov rcx, [rsp + 16]
                ; mov rax, [rsp + 8]
            );
            f(assembler, value_stack, Register::RCX, Register::RAX);
            dynasm!(
                assembler
                ; mov [rsp + 16], rcx
                ; pop rcx
                ; add rsp, 8
            );
        }

        Ok(())
    }

    fn emit_binop_i32<F: FnOnce(&mut Assembler, &ValueStack, Register, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
    ) -> Result<(), CodegenError> {
        Self::emit_binop(assembler, value_stack, f, WpType::I32, WpType::I32)
    }

    fn emit_binop_i64<F: FnOnce(&mut Assembler, &ValueStack, Register, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
    ) -> Result<(), CodegenError> {
        Self::emit_binop(assembler, value_stack, f, WpType::I64, WpType::I64)
    }

    fn emit_shift<F: FnOnce(&mut Assembler, Register)>(
        assembler: &mut Assembler,
        value_stack: &ValueStack,
        left: Register,
        right: Register,
        f: F,
    ) {
        let rcx_used = Register::RCX.is_used(value_stack);
        if rcx_used {
            dynasm!(
                assembler
                ; push rcx
            );
        }
        dynasm!(
            assembler
            ; mov rcx, Rq(right as u8)
        );
        f(assembler, left);
        if rcx_used {
            dynasm!(
                assembler
                ; pop rcx
            );
        }
    }

    fn emit_div_i32(
        assembler: &mut Assembler,
        value_stack: &ValueStack,
        left: Register,
        right: Register,
        signed: bool,
        out: Register,
    ) {
        let dx_save =
            Register::RDX.is_used(value_stack) && left != Register::RDX && right != Register::RDX;
        if dx_save {
            dynasm!(
                assembler
                ; push rdx
            );
        }

        dynasm!(
            assembler
            ; push r15
            ; mov r15d, Rd(right as u8)
            ; mov eax, Rd(left as u8)
        );
        if signed {
            dynasm!(
                assembler
                ; cdq
                ; idiv r15d
            );
        } else {
            dynasm!(
                assembler
                ; xor edx, edx
                ; div r15d
            );
        }
        dynasm!(
            assembler
            ; mov Rd(left as u8), Rd(out as u8)
            ; pop r15
        );

        if dx_save {
            dynasm!(
                assembler
                ; pop rdx
            );
        }
    }

    fn emit_div_i64(
        assembler: &mut Assembler,
        value_stack: &ValueStack,
        left: Register,
        right: Register,
        signed: bool,
        out: Register,
    ) {
        let dx_save =
            Register::RDX.is_used(value_stack) && left != Register::RDX && right != Register::RDX;
        if dx_save {
            dynasm!(
                assembler
                ; push rdx
            );
        }

        dynasm!(
            assembler
            ; push r15
            ; mov r15, Rq(right as u8)
            ; mov rax, Rq(left as u8)
        );
        if signed {
            dynasm!(
                assembler
                ; cqo
                ; idiv r15
            );
        } else {
            dynasm!(
                assembler
                ; xor rdx, rdx
                ; div r15
            );
        }
        dynasm!(
            assembler
            ; mov Rq(left as u8), Rq(out as u8)
            ; pop r15
        );

        if dx_save {
            dynasm!(
                assembler
                ; pop rdx
            );
        }
    }

    fn emit_cmp_i32<F: FnOnce(&mut Assembler)>(
        assembler: &mut Assembler,
        left: Register,
        right: Register,
        f: F,
    ) {
        dynasm!(
            assembler
            ; cmp Rd(left as u8), Rd(right as u8)
        );
        f(assembler);
        dynasm!(
            assembler
            ; xor Rd(left as u8), Rd(left as u8)
            ; jmp >label_end
            ; label_true:
            ; mov Rd(left as u8), 1
            ; label_end:
        );
    }

    fn emit_cmp_i64<F: FnOnce(&mut Assembler)>(
        assembler: &mut Assembler,
        left: Register,
        right: Register,
        f: F,
    ) {
        dynasm!(
            assembler
            ; cmp Rq(left as u8), Rq(right as u8)
        );
        f(assembler);
        dynasm!(
            assembler
            ; xor Rq(left as u8), Rq(left as u8)
            ; jmp >label_end
            ; label_true:
            ; mov Rq(left as u8), 1
            ; label_end:
        );
    }

    fn emit_peek_into_ax(
        assembler: &mut Assembler,
        value_stack: &ValueStack,
    ) -> Result<WpType, CodegenError> {
        let val = match value_stack.values.last() {
            Some(x) => *x,
            None => {
                return Err(CodegenError {
                    message: "no value",
                });
            }
        };
        match val.location {
            ValueLocation::Register(x) => {
                let reg = Register::from_scratch_reg(x);
                dynasm!(
                    assembler
                    ; mov rax, Rq(reg as u8)
                );
            }
            ValueLocation::Stack => {
                dynasm!(
                    assembler
                    ; mov rax, [rsp]
                );
            }
        }

        Ok(val.ty)
    }

    fn emit_pop_into_reg(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        target: Register,
    ) -> Result<WpType, CodegenError> {
        let val = value_stack.pop()?;
        match val.location {
            ValueLocation::Register(x) => {
                let reg = Register::from_scratch_reg(x);
                dynasm!(
                    assembler
                    ; mov Rq(target as u8), Rq(reg as u8)
                );
            }
            ValueLocation::Stack => {
                dynasm!(
                    assembler
                    ; pop Rq(target as u8)
                );
            }
        }

        Ok(val.ty)
    }

    fn emit_pop_into_ax(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
    ) -> Result<WpType, CodegenError> {
        Self::emit_pop_into_reg(assembler, value_stack, Register::RAX)
    }

    fn emit_push_from_reg(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        ty: WpType,
        source: Register,
    ) -> Result<(), CodegenError> {
        let loc = value_stack.push(ty);
        match loc {
            ValueLocation::Register(x) => {
                let reg = Register::from_scratch_reg(x);
                dynasm!(
                    assembler
                    ; mov Rq(reg as u8), Rq(source as u8)
                );
            }
            ValueLocation::Stack => {
                dynasm!(
                    assembler
                    ; push Rq(source as u8)
                );
            }
        }

        Ok(())
    }

    fn emit_push_from_ax(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        ty: WpType,
    ) -> Result<(), CodegenError> {
        Self::emit_push_from_reg(assembler, value_stack, ty, Register::RAX)
    }

    fn emit_leave_frame(
        assembler: &mut Assembler,
        frame: &ControlFrame,
        value_stack: &mut ValueStack,
        peek: bool,
    ) -> Result<(), CodegenError> {
        let ret_ty = match frame.returns.len() {
            1 => Some(frame.returns[0]),
            0 => None,
            _ => {
                return Err(CodegenError {
                    message: "more than one block returns are not yet supported",
                });
            }
        };

        if value_stack.values.len() < frame.value_stack_depth_before + frame.returns.len() {
            return Err(CodegenError {
                message: "value stack underflow",
            });
        }

        if let Some(_) = ret_ty {
            if value_stack.values.iter().last().map(|x| x.ty) != ret_ty {
                return Err(CodegenError {
                    message: "value type != return type",
                });
            }
            if peek {
                Self::emit_peek_into_ax(assembler, value_stack)?;
            } else {
                Self::emit_pop_into_ax(assembler, value_stack)?;
            }
        }

        Ok(())
    }

    fn emit_else(
        assembler: &mut Assembler,
        control_stack: &mut ControlStack,
        value_stack: &mut ValueStack,
        was_unreachable: bool,
    ) -> Result<(), CodegenError> {
        let frame = match control_stack.frames.last_mut() {
            Some(x) => x,
            None => {
                return Err(CodegenError {
                    message: "no frame (else)",
                });
            }
        };

        if !was_unreachable {
            Self::emit_leave_frame(assembler, frame, value_stack, false)?;
            if value_stack.values.len() != frame.value_stack_depth_before {
                return Err(CodegenError {
                    message: "value_stack.values.len() != frame.value_stack_depth_before",
                });
            }
        } else {
            // No need to actually unwind the stack here.
            value_stack.reset_depth(frame.value_stack_depth_before);
        }

        match frame.if_else {
            IfElseState::If(label) => {
                dynasm!(
                    assembler
                    ; jmp =>frame.label
                    ; => label
                );
                frame.if_else = IfElseState::Else;
            }
            _ => {
                return Err(CodegenError {
                    message: "unexpected if else state",
                });
            }
        }

        Ok(())
    }

    fn emit_block_end(
        assembler: &mut Assembler,
        control_stack: &mut ControlStack,
        value_stack: &mut ValueStack,
        was_unreachable: bool,
    ) -> Result<(), CodegenError> {
        let frame = match control_stack.frames.pop() {
            Some(x) => x,
            None => {
                return Err(CodegenError {
                    message: "no frame (block end)",
                });
            }
        };

        if !was_unreachable {
            Self::emit_leave_frame(assembler, &frame, value_stack, false)?;
            if value_stack.values.len() != frame.value_stack_depth_before {
                return Err(CodegenError {
                    message: "value_stack.values.len() != frame.value_stack_depth_before",
                });
            }
        } else {
            // No need to actually unwind the stack here.
            value_stack.reset_depth(frame.value_stack_depth_before);
        }

        if !frame.loop_like {
            match frame.if_else {
                IfElseState::None | IfElseState::Else => {
                    dynasm!(
                        assembler
                        ; => frame.label
                    );
                }
                IfElseState::If(label) => {
                    dynasm!(
                        assembler
                        ; => frame.label
                        ; => label
                    );

                    if frame.returns.len() != 0 {
                        return Err(CodegenError {
                            message: "if without else, with non-empty returns",
                        });
                    }
                }
            }
        }

        if frame.returns.len() == 1 {
            let loc = value_stack.push(frame.returns[0]);
            match loc {
                ValueLocation::Register(x) => {
                    let reg = Register::from_scratch_reg(x);
                    dynasm!(
                        assembler
                        ; mov Rq(reg as u8), rax
                    );
                }
                ValueLocation::Stack => {
                    dynasm!(
                        assembler
                        ; push rax
                    );
                }
            }
        }

        Ok(())
    }

    fn emit_jmp(
        assembler: &mut Assembler,
        control_stack: &ControlStack,
        value_stack: &mut ValueStack,
        relative_frame_offset: usize,
    ) -> Result<(), CodegenError> {
        let frame = if relative_frame_offset >= control_stack.frames.len() {
            return Err(CodegenError {
                message: "jmp offset out of bounds",
            });
        } else {
            &control_stack.frames[control_stack.frames.len() - 1 - relative_frame_offset]
        };

        if !frame.loop_like {
            Self::emit_leave_frame(assembler, frame, value_stack, true)?;
        }

        let mut sp_diff: usize = 0;

        for i in 0..value_stack.values.len() - frame.value_stack_depth_before {
            let vi = value_stack.values[value_stack.values.len() - 1 - i];
            if vi.location == ValueLocation::Stack {
                sp_diff += 8
            } else {
                break;
            }
        }

        dynasm!(
            assembler
            ; add rsp, sp_diff as i32
            ; jmp =>frame.label
        );

        Ok(())
    }

    fn emit_return(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        returns: &Vec<WpType>,
    ) -> Result<(), CodegenError> {
        match returns.len() {
            0 => {}
            1 => {
                if value_stack.values.iter().last().map(|x| x.ty) != Some(returns[0]) {
                    return Err(CodegenError {
                        message: "self.value_stack.last().cloned() != Some(self.returns[0])",
                    });
                }
                Self::emit_pop_into_ax(assembler, value_stack)?;
            }
            _ => {
                return Err(CodegenError {
                    message: "multiple return values is not yet supported",
                });
            }
        }

        dynasm!(
            assembler
            ; mov rsp, rbp
            ; pop rbp
            ; ret
        );

        Ok(())
    }

    fn emit_update_memory_from_ctx(
        assembler: &mut Assembler,
        info: &ModuleInfo,
    ) -> Result<(), CodegenError> {
        if info.memories.len() > 0 {
            if info.memories.len() != 1 || info.imported_memories.len() != 0 {
                return Err(CodegenError {
                    message: "only one linear memory is supported",
                });
            }
            dynasm!(
                assembler
                ; mov r15, r14 => vm::InternalCtx.memories
            );
        } else if info.imported_memories.len() > 0 {
            if info.memories.len() != 0 || info.imported_memories.len() != 1 {
                return Err(CodegenError {
                    message: "only one linear memory is supported",
                });
            }
            dynasm!(
                assembler
                ; mov r15, r14 => vm::InternalCtx.imported_memories
            );
        } else {
            return Ok(());
        };

        dynasm!(
            assembler
            ; mov r15, [r15]
            ; mov r13, r15 => LocalMemory.bound
            ; shr r13, 16 // 65536 bytes per page
            ; shl r13, 48
            ; mov r15, r15 => LocalMemory.base
            ; or r15, r13
        );
        Ok(())
    }

    fn emit_managed_call_trampoline(
        assembler: &mut Assembler,
        info: &ModuleInfo,
        target: FuncPtr,
        num_params: usize,
    ) -> Result<(), CodegenError> {
        dynasm!(
            assembler
            ; push rbp
            ; mov rbp, rsp
        );

        for i in 0..num_params {
            match i {
                i if i < 5 => {
                    let reg = match i {
                        0 => Register::RSI,
                        1 => Register::RDX,
                        2 => Register::RCX,
                        3 => Register::R8,
                        4 => Register::R9,
                        _ => unreachable!(),
                    };
                    dynasm!(
                        assembler
                        ; push Rq(reg as u8)
                    );
                }
                i => {
                    let offset = (i - 5) * 8;
                    dynasm!(
                        assembler
                        ; mov rax, [rbp + (16 + offset) as i32]
                        ; push rax
                    );
                }
            }
        }

        dynasm!(
            assembler
            ; mov r9, rdi // vmctx
            ; mov rdx, QWORD target.0 as usize as i64
            ; mov rsi, QWORD (num_params * 8) as i64
            ; mov rdi, rsp
        );

        let has_memory = if info.memories.len() > 0 {
            if info.memories.len() != 1 || info.imported_memories.len() != 0 {
                return Err(CodegenError {
                    message: "only one linear memory is supported",
                });
            }
            dynasm!(
                assembler
                ; mov rcx, r9 => vm::InternalCtx.memories
            );
            true
        } else if info.imported_memories.len() > 0 {
            if info.memories.len() != 0 || info.imported_memories.len() != 1 {
                return Err(CodegenError {
                    message: "only one linear memory is supported",
                });
            }
            dynasm!(
                assembler
                ; mov rcx, r9 => vm::InternalCtx.imported_memories
            );
            true
        } else {
            false
        };

        if has_memory {
            dynasm!(
                assembler
                ; mov rcx, [rcx]
                ; mov r8, rcx => LocalMemory.bound
                ; shr r8, 16 // 65536 bytes per page
                ; mov rcx, rcx => LocalMemory.base
            );
        } else {
            dynasm!(
                assembler
                ; mov rcx, 0
            );
        }

        dynasm!(
            assembler
            ; mov rax, QWORD *CALL_WASM as usize as i64
            ; call rax
            ; mov rsp, rbp
            ; pop rbp
            ; ret
        );

        Ok(())
    }

    fn emit_f32_int_conv_check(
        assembler: &mut Assembler,
        reg: Register,
        lower_bound: f32,
        upper_bound: f32,
    ) {
        let lower_bound = f32::to_bits(lower_bound);
        let upper_bound = f32::to_bits(upper_bound);

        dynasm!(
            assembler
            ; movq xmm5, r15

            // underflow
            ; movd xmm1, Rd(reg as u8)
            ; mov r15d, lower_bound as i32
            ; movd xmm2, r15d
            ; vcmpltss xmm0, xmm1, xmm2
            ; movd r15d, xmm0
            ; cmp r15d, 1
            ; je >trap

            // overflow
            ; mov r15d, upper_bound as i32
            ; movd xmm2, r15d
            ; vcmpgtss xmm0, xmm1, xmm2
            ; movd r15d, xmm0
            ; cmp r15d, 1
            ; je >trap

            // NaN
            ; vcmpeqss xmm0, xmm1, xmm1
            ; movd r15d, xmm0
            ; cmp r15d, 0
            ; je >trap

            ; movq r15, xmm5
            ; jmp >ok

            ; trap:
            ; ud2

            ; ok:
        );
    }

    fn emit_f64_int_conv_check(
        assembler: &mut Assembler,
        reg: Register,
        lower_bound: f64,
        upper_bound: f64,
    ) {
        let lower_bound = f64::to_bits(lower_bound);
        let upper_bound = f64::to_bits(upper_bound);

        dynasm!(
            assembler
            ; movq xmm5, r15

            // underflow
            ; movq xmm1, Rq(reg as u8)
            ; mov r15, QWORD lower_bound as i64
            ; movq xmm2, r15
            ; vcmpltsd xmm0, xmm1, xmm2
            ; movd r15d, xmm0
            ; cmp r15d, 1
            ; je >trap

            // overflow
            ; mov r15, QWORD upper_bound as i64
            ; movq xmm2, r15
            ; vcmpgtsd xmm0, xmm1, xmm2
            ; movd r15d, xmm0
            ; cmp r15d, 1
            ; je >trap

            // NaN
            ; vcmpeqsd xmm0, xmm1, xmm1
            ; movd r15d, xmm0
            ; cmp r15d, 0
            ; je >trap

            ; movq r15, xmm5
            ; jmp >ok

            ; trap:
            ; ud2

            ; ok:
        );
    }

    fn emit_native_call_trampoline<A: Copy + Sized, B: Copy + Sized>(
        assembler: &mut Assembler,
        target: unsafe extern "C" fn(
            ctx1: A,
            ctx2: B,
            stack_top: *mut u8,
            stack_base: *mut u8,
            vmctx: *mut vm::Ctx,
            memory_base: *mut u8,
        ) -> u64,
        ctx1: A,
        ctx2: B,
    ) -> DynamicLabel {
        let label = assembler.new_dynamic_label();

        dynasm!(
            assembler
            ; =>label
        );

        // FIXME: Check at compile time.
        assert_eq!(::std::mem::size_of::<A>(), ::std::mem::size_of::<i64>());
        assert_eq!(::std::mem::size_of::<B>(), ::std::mem::size_of::<i64>());

        dynasm!(
            assembler
            ; mov rdi, QWORD unsafe { ::std::mem::transmute_copy::<A, i64>(&ctx1) }
            ; mov rsi, QWORD unsafe { ::std::mem::transmute_copy::<B, i64>(&ctx2) }
            ; mov rdx, rsp
            ; mov rcx, rbp
            ; mov r8, r14 // vmctx
            ; mov r9, r15 // memory_base
            ; mov rax, QWORD 0xfffffffffffffff0u64 as i64
            ; and rsp, rax
            ; mov rax, QWORD target as i64
            ; call rax
            ; mov rsp, rbp
            ; pop rbp
            ; ret
        );

        label
    }

    fn emit_call_raw(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        target: DynamicLabel,
        params: &[WpType],
        returns: &[WpType],
    ) -> Result<(), CodegenError> {
        let total_size: usize = params.len() * 8;

        if params.len() > value_stack.values.len() {
            return Err(CodegenError {
                message: "value stack underflow in call",
            });
        }

        let mut saved_regs: Vec<Register> = Vec::new();

        for v in &value_stack.values[0..value_stack.values.len() - params.len()] {
            match v.location {
                ValueLocation::Register(x) => {
                    let reg = Register::from_scratch_reg(x);
                    dynasm!(
                        assembler
                        ; push Rq(reg as u8)
                    );
                    saved_regs.push(reg);
                }
                ValueLocation::Stack => break,
            }
        }

        dynasm!(
            assembler
            ; lea rax, [>after_call] // TODO: Is this correct?
            ; push rax
            ; push rbp
        );

        if total_size != 0 {
            dynasm!(
                assembler
                ; sub rsp, total_size as i32
            );
        }

        let mut offset: usize = 0;
        let mut caller_stack_offset: usize = 0;
        for ty in params.iter().rev() {
            let val = value_stack.pop()?;
            if val.ty != *ty {
                return Err(CodegenError {
                    message: "value type mismatch in call",
                });
            }

            match val.location {
                ValueLocation::Register(x) => {
                    let reg = Register::from_scratch_reg(x);
                    dynasm!(
                        assembler
                        ; mov [rsp + offset as i32], Rq(reg as u8)
                    );
                }
                ValueLocation::Stack => {
                    dynasm!(
                        assembler
                        ; mov rax, [rsp + (total_size + 16 + saved_regs.len() * 8 + caller_stack_offset) as i32]
                        ; mov [rsp + offset as i32], rax
                    );
                    caller_stack_offset += 8;
                }
            }

            offset += 8;
        }

        assert_eq!(offset, total_size);

        dynasm!(
            assembler
            ; mov rbp, rsp
        );
        if total_size != 0 {
            dynasm!(
                assembler
                ; add rbp, total_size as i32
            );
        }
        dynasm!(
            assembler
            ; jmp =>target
            ; after_call:
        );

        for reg in saved_regs.iter().rev() {
            dynasm!(
                assembler
                ; pop Rq(*reg as u8)
            );
        }

        if caller_stack_offset != 0 {
            dynasm!(
                assembler
                ; add rsp, caller_stack_offset as i32
            );
        }

        match returns.len() {
            0 => {}
            1 => {
                Self::emit_push_from_ax(assembler, value_stack, returns[0])?;
            }
            _ => {
                return Err(CodegenError {
                    message: "more than 1 function returns are not supported",
                });
            }
        }

        Ok(())
    }

    fn emit_memory_bound_check_if_needed(
        assembler: &mut Assembler,
        module_info: &ModuleInfo,
        offset_reg: Register,
        value_size: usize,
    ) {
        let mem_desc = match MemoryIndex::new(0).local_or_import(module_info) {
            LocalOrImport::Local(local_mem_index) => &module_info.memories[local_mem_index],
            LocalOrImport::Import(import_mem_index) => {
                &module_info.imported_memories[import_mem_index].1
            }
        };
        let need_check = match mem_desc.memory_type() {
            MemoryType::Dynamic => true,
            MemoryType::Static | MemoryType::SharedStatic => false,
        };
        if need_check {
            dynasm!(
                assembler
                ; movq xmm5, r14
                ; lea r14, [Rq(offset_reg as u8) + value_size as i32] // overflow isn't possible since offset_reg contains a 32-bit value.

                ; mov r13, r15
                ; shr r13, 48
                ; shl r13, 16
                ; cmp r14, r13
                ; ja >out_of_bounds
                ; jmp >ok

                ; out_of_bounds:
                ; ud2
                ; ok:
                ; movq r14, xmm5
            );
        }
    }

    fn emit_memory_load<F: FnOnce(&mut Assembler, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
        out_ty: WpType,
        module_info: &ModuleInfo,
        read_size: usize,
    ) -> Result<(), CodegenError> {
        let addr_info = value_stack.pop()?;
        let out_loc = value_stack.push(out_ty);

        if addr_info.ty != WpType::I32 {
            return Err(CodegenError {
                message: "memory address must be i32",
            });
        }

        assert_eq!(out_loc, addr_info.location);

        match addr_info.location {
            ValueLocation::Register(x) => {
                let reg = Register::from_scratch_reg(x);
                dynasm!(
                    assembler
                    ; mov Rd(reg as u8), Rd(reg as u8)
                );
                Self::emit_memory_bound_check_if_needed(assembler, module_info, reg, read_size);
                dynasm!(
                    assembler
                    ; add Rq(reg as u8), r15
                    ; shl Rq(reg as u8), 16
                    ; shr Rq(reg as u8), 16
                );
                f(assembler, reg);
            }
            ValueLocation::Stack => {
                dynasm!(
                    assembler
                    ; pop rax
                    ; mov eax, eax
                );
                Self::emit_memory_bound_check_if_needed(
                    assembler,
                    module_info,
                    Register::RAX,
                    read_size,
                );
                dynasm!(
                    assembler
                    ; add rax, r15
                    ; shl rax, 16
                    ; shr rax, 16
                );
                f(assembler, Register::RAX);
                dynasm!(
                    assembler
                    ; push rax
                )
            }
        }
        Ok(())
    }

    fn emit_memory_store<F: FnOnce(&mut Assembler, Register, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
        value_ty: WpType,
        module_info: &ModuleInfo,
        write_size: usize,
    ) -> Result<(), CodegenError> {
        let value_info = value_stack.pop()?;
        let addr_info = value_stack.pop()?;

        if addr_info.ty != WpType::I32 {
            return Err(CodegenError {
                message: "memory address must be i32",
            });
        }

        if value_info.ty != value_ty {
            return Err(CodegenError {
                message: "value type mismatch in memory store",
            });
        }

        match value_info.location {
            ValueLocation::Register(x) => {
                let value_reg = Register::from_scratch_reg(x);
                let addr_reg =
                    Register::from_scratch_reg(addr_info.location.get_register().unwrap()); // must be a register
                dynasm!(
                    assembler
                    ; mov Rd(addr_reg as u8), Rd(addr_reg as u8)
                );
                Self::emit_memory_bound_check_if_needed(
                    assembler,
                    module_info,
                    addr_reg,
                    write_size,
                );
                dynasm!(
                    assembler
                    ; add Rq(addr_reg as u8), r15
                    ; shl Rq(addr_reg as u8), 16
                    ; shr Rq(addr_reg as u8), 16
                );
                f(assembler, addr_reg, value_reg);
            }
            ValueLocation::Stack => {
                match addr_info.location {
                    ValueLocation::Register(x) => {
                        let addr_reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; mov Rd(addr_reg as u8), Rd(addr_reg as u8)
                        );
                        Self::emit_memory_bound_check_if_needed(
                            assembler,
                            module_info,
                            addr_reg,
                            write_size,
                        );
                        dynasm!(
                            assembler
                            ; add Rq(addr_reg as u8), r15
                            ; shl Rq(addr_reg as u8), 16
                            ; shr Rq(addr_reg as u8), 16
                            ; pop rax
                        );
                        f(assembler, addr_reg, Register::RAX);
                    }
                    ValueLocation::Stack => {
                        dynasm!(
                            assembler
                            ; mov [rsp - 8], rcx // red zone
                            ; pop rax // value
                            ; pop rcx // address
                        );
                        dynasm!(
                            assembler
                            ; mov ecx, ecx
                        );
                        Self::emit_memory_bound_check_if_needed(
                            assembler,
                            module_info,
                            Register::RCX,
                            write_size,
                        );
                        dynasm!(
                            assembler
                            ; add rcx, r15
                            ; shl rcx, 16
                            ; shr rcx, 16
                        );
                        f(assembler, Register::RCX, Register::RAX);
                        dynasm!(
                            assembler
                            ; mov rcx, [rsp - 24]
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

impl FunctionCodeGenerator for X64FunctionCode {
    fn feed_return(&mut self, ty: WpType) -> Result<(), CodegenError> {
        self.returns.push(ty);
        Ok(())
    }

    /// Stack layout of a call frame:
    /// - Return address
    /// - Old RBP
    /// - Params in reversed order, caller initialized
    /// - Locals in reversed order, callee initialized
    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError> {
        self.current_stack_offset += 8;
        self.locals.push(Local {
            ty: ty,
            stack_offset: self.current_stack_offset,
        });

        self.num_params += 1;

        Ok(())
    }

    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError> {
        let assembler = self.assembler.as_mut().unwrap();
        let size = get_size_of_type(&ty)?;

        if is_dword(size) {
            for _ in 0..n {
                // FIXME: check range of n
                self.current_stack_offset += 4;
                self.locals.push(Local {
                    ty: ty,
                    stack_offset: self.current_stack_offset,
                });
                dynasm!(
                    assembler
                    ; sub rsp, 4
                    ; mov DWORD [rsp], 0
                );
            }
            if n % 2 == 1 {
                self.current_stack_offset += 4;
                dynasm!(
                    assembler
                    ; sub rsp, 4
                );
            }
        } else {
            for _ in 0..n {
                // FIXME: check range of n
                self.current_stack_offset += 8;
                self.locals.push(Local {
                    ty: ty,
                    stack_offset: self.current_stack_offset,
                });
                dynasm!(
                    assembler
                    ; push 0
                );
            }
        }
        Ok(())
    }
    fn begin_body(&mut self) -> Result<(), CodegenError> {
        self.control_stack = Some(ControlStack::new(
            self.assembler.as_mut().unwrap().new_dynamic_label(),
            self.returns.clone(),
        ));
        Ok(())
    }
    fn feed_opcode(&mut self, op: Operator, module_info: &ModuleInfo) -> Result<(), CodegenError> {
        let was_unreachable;

        if self.unreachable_depth > 0 {
            was_unreachable = true;
            match op {
                Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                    self.unreachable_depth += 1;
                }
                Operator::End => {
                    self.unreachable_depth -= 1;
                }
                Operator::Else => {
                    // We are in a reachable true branch
                    if self.unreachable_depth == 1 {
                        if let Some(IfElseState::If(_)) = self
                            .control_stack
                            .as_ref()
                            .unwrap()
                            .frames
                            .last()
                            .map(|x| x.if_else)
                        {
                            self.unreachable_depth -= 1;
                        }
                    }
                }
                _ => {}
            }
            if self.unreachable_depth > 0 {
                return Ok(());
            }
        } else {
            was_unreachable = false;
        }

        let assembler = self.assembler.as_mut().unwrap();

        match op {
            Operator::GetGlobal { global_index } => {
                let mut global_index = global_index as usize;
                if global_index < module_info.imported_globals.len() {
                    dynasm!(
                        assembler
                        ; mov rax, r14 => vm::InternalCtx.imported_globals
                    );
                } else {
                    global_index -= module_info.imported_globals.len();
                    if global_index >= module_info.globals.len() {
                        return Err(CodegenError {
                            message: "global out of bounds",
                        });
                    }
                    dynasm!(
                        assembler
                        ; mov rax, r14 => vm::InternalCtx.globals
                    );
                }

                dynasm!(
                    assembler
                    ; mov rax, [rax + (global_index as i32) * 8]
                    ; mov rax, rax => LocalGlobal.data
                );
                Self::emit_push_from_ax(
                    assembler,
                    &mut self.value_stack,
                    type_to_wp_type(
                        module_info.globals[LocalGlobalIndex::new(global_index)]
                            .desc
                            .ty,
                    ),
                )?;
            }
            Operator::SetGlobal { global_index } => {
                let ty = Self::emit_pop_into_ax(assembler, &mut self.value_stack)?;

                let mut global_index = global_index as usize;
                if global_index < module_info.imported_globals.len() {
                    dynasm!(
                        assembler
                        ; push rbx
                        ; mov rbx, r14 => vm::InternalCtx.imported_globals
                    );
                } else {
                    global_index -= module_info.imported_globals.len();
                    if global_index >= module_info.globals.len() {
                        return Err(CodegenError {
                            message: "global out of bounds",
                        });
                    }
                    dynasm!(
                        assembler
                        ; push rbx
                        ; mov rbx, r14 => vm::InternalCtx.globals
                    );
                }

                if ty
                    != type_to_wp_type(
                        module_info.globals[LocalGlobalIndex::new(global_index)]
                            .desc
                            .ty,
                    )
                {
                    return Err(CodegenError {
                        message: "type mismatch in SetGlobal",
                    });
                }
                dynasm!(
                    assembler
                    ; mov rbx, [rbx + (global_index as i32) * 8]
                    ; mov rbx => LocalGlobal.data, rax
                    ; pop rbx
                );
            }
            Operator::GetLocal { local_index } => {
                let local_index = local_index as usize;
                if local_index >= self.locals.len() {
                    return Err(CodegenError {
                        message: "local out of bounds",
                    });
                }
                let local = self.locals[local_index];
                let location = self.value_stack.push(local.ty);
                let size = get_size_of_type(&local.ty)?;

                match location {
                    ValueLocation::Register(id) => {
                        if is_dword(size) {
                            dynasm!(
                                assembler
                                ; mov Rd(Register::from_scratch_reg(id) as u8), [rbp - (local.stack_offset as i32)]
                            );
                        } else {
                            dynasm!(
                                assembler
                                ; mov Rq(Register::from_scratch_reg(id) as u8), [rbp - (local.stack_offset as i32)]
                            );
                        }
                    }
                    ValueLocation::Stack => {
                        if is_dword(size) {
                            dynasm!(
                                assembler
                                ; mov eax, [rbp - (local.stack_offset as i32)]
                                ; push rax
                            );
                        } else {
                            dynasm!(
                                assembler
                                ; mov rax, [rbp - (local.stack_offset as i32)]
                                ; push rax
                            );
                        }
                    }
                }
            }
            Operator::SetLocal { local_index } => {
                let local_index = local_index as usize;
                if local_index >= self.locals.len() {
                    return Err(CodegenError {
                        message: "local out of bounds",
                    });
                }
                let local = self.locals[local_index];
                let ty = Self::emit_pop_into_ax(assembler, &mut self.value_stack)?;
                if ty != local.ty {
                    return Err(CodegenError {
                        message: "SetLocal type mismatch",
                    });
                }

                if is_dword(get_size_of_type(&ty)?) {
                    dynasm!(
                        assembler
                        ; mov [rbp - (local.stack_offset as i32)], eax
                    );
                } else {
                    dynasm!(
                        assembler
                        ; mov [rbp - (local.stack_offset as i32)], rax
                    );
                }
            }
            Operator::TeeLocal { local_index } => {
                let local_index = local_index as usize;
                if local_index >= self.locals.len() {
                    return Err(CodegenError {
                        message: "local out of bounds",
                    });
                }
                let local = self.locals[local_index];
                let ty = Self::emit_peek_into_ax(assembler, &self.value_stack)?;
                if ty != local.ty {
                    return Err(CodegenError {
                        message: "TeeLocal type mismatch",
                    });
                }

                if is_dword(get_size_of_type(&ty)?) {
                    dynasm!(
                        assembler
                        ; mov [rbp - (local.stack_offset as i32)], eax
                    );
                } else {
                    dynasm!(
                        assembler
                        ; mov [rbp - (local.stack_offset as i32)], rax
                    );
                }
            }
            Operator::I32Const { value } => {
                let location = self.value_stack.push(WpType::I32);
                match location {
                    ValueLocation::Register(x) => {
                        let reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), value
                        );
                    }
                    ValueLocation::Stack => {
                        dynasm!(
                            assembler
                            ; push value
                        );
                    }
                }
            }
            Operator::I32Add => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; add Rd(left as u8), Rd(right as u8)
                        )
                    },
                )?;
            }
            Operator::I32Sub => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; sub Rd(left as u8), Rd(right as u8)
                        )
                    },
                )?;
            }
            Operator::I32Mul => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; imul Rd(left as u8), Rd(right as u8)
                        )
                    },
                )?;
            }
            Operator::I32DivU => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i32(
                            assembler,
                            value_stack,
                            left,
                            right,
                            false,
                            Register::RAX,
                        );
                    },
                )?;
            }
            Operator::I32DivS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i32(
                            assembler,
                            value_stack,
                            left,
                            right,
                            true,
                            Register::RAX,
                        );
                    },
                )?;
            }
            Operator::I32RemU => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i32(
                            assembler,
                            value_stack,
                            left,
                            right,
                            false,
                            Register::RDX,
                        );
                    },
                )?;
            }
            Operator::I32RemS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i32(
                            assembler,
                            value_stack,
                            left,
                            right,
                            true,
                            Register::RDX,
                        );
                    },
                )?;
            }
            Operator::I32And => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; and Rd(left as u8), Rd(right as u8)
                        );
                    },
                )?;
            }
            Operator::I32Or => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; or Rd(left as u8), Rd(right as u8)
                        );
                    },
                )?;
            }
            Operator::I32Xor => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; xor Rd(left as u8), Rd(right as u8)
                        );
                    },
                )?;
            }
            Operator::I32Eq => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; cmp Rd(left as u8), Rd(right as u8)
                            ; lahf
                            ; shr ax, 14
                            ; and eax, 1
                            ; mov Rd(left as u8), eax
                        );
                    },
                )?;
            }
            Operator::I32Ne => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; cmp Rd(left as u8), Rd(right as u8)
                            ; lahf
                            ; shr ax, 14
                            ; and eax, 1
                            ; xor eax, 1
                            ; mov Rd(left as u8), eax
                        );
                    },
                )?;
            }
            Operator::I32Eqz => {
                Self::emit_unop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; cmp Rd(reg as u8), 0
                            ; lahf
                            ; shr ax, 14
                            ; and eax, 1
                        );
                        if reg != Register::RAX {
                            dynasm!(
                                assembler
                                ; mov Rd(reg as u8), eax
                            );
                        }
                    },
                )?;
            }
            Operator::I32Clz => {
                Self::emit_unop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; lzcnt Rd(reg as u8), Rd(reg as u8)
                        );
                    },
                )?;
            }
            Operator::I32Ctz => {
                Self::emit_unop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; tzcnt Rd(reg as u8), Rd(reg as u8)
                        );
                    },
                )?;
            }
            Operator::I32Popcnt => {
                Self::emit_unop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; popcnt Rd(reg as u8), Rd(reg as u8)
                        );
                    },
                )?;
            }
            Operator::I32Shl => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; shl Rd(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I32ShrU => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; shr Rd(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I32ShrS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; sar Rd(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I32Rotl => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; rol Rd(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I32Rotr => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; ror Rd(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            // Comparison operators.
            // https://en.wikibooks.org/wiki/X86_Assembly/Control_Flow
            // TODO: Is reading flag register directly faster?
            Operator::I32LtS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jl >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I32LeS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jle >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I32GtS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jg >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I32GeS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jge >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I32LtU => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jb >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I32LeU => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jbe >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I32GtU => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; ja >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I32GeU => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jae >label_true
                            );
                        });
                    },
                )?;
            }
            Operator::I64Const { value } => {
                let location = self.value_stack.push(WpType::I64);
                match location {
                    ValueLocation::Register(x) => {
                        let reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; mov Rq(reg as u8), QWORD value
                        );
                    }
                    ValueLocation::Stack => {
                        dynasm!(
                            assembler
                            ; mov rax, QWORD value
                            ; push rax
                        );
                    }
                }
            }
            Operator::I64Add => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; add Rq(left as u8), Rq(right as u8)
                        )
                    },
                )?;
            }
            Operator::I64Sub => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; sub Rq(left as u8), Rq(right as u8)
                        )
                    },
                )?;
            }
            Operator::I64Mul => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; imul Rq(left as u8), Rq(right as u8)
                        )
                    },
                )?;
            }
            Operator::I64DivU => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i64(
                            assembler,
                            value_stack,
                            left,
                            right,
                            false,
                            Register::RAX,
                        );
                    },
                )?;
            }
            Operator::I64DivS => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i64(
                            assembler,
                            value_stack,
                            left,
                            right,
                            true,
                            Register::RAX,
                        );
                    },
                )?;
            }
            Operator::I64RemU => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i64(
                            assembler,
                            value_stack,
                            left,
                            right,
                            false,
                            Register::RDX,
                        );
                    },
                )?;
            }
            Operator::I64RemS => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_div_i64(
                            assembler,
                            value_stack,
                            left,
                            right,
                            true,
                            Register::RDX,
                        );
                    },
                )?;
            }
            Operator::I64And => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; and Rq(left as u8), Rq(right as u8)
                        );
                    },
                )?;
            }
            Operator::I64Or => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; or Rq(left as u8), Rq(right as u8)
                        );
                    },
                )?;
            }
            Operator::I64Xor => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; xor Rq(left as u8), Rq(right as u8)
                        );
                    },
                )?;
            }
            Operator::I64Eq => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; cmp Rq(left as u8), Rq(right as u8)
                            ; lahf
                            ; shr ax, 14
                            ; and eax, 1
                            ; mov Rd(left as u8), eax
                        );
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64Ne => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; cmp Rq(left as u8), Rq(right as u8)
                            ; lahf
                            ; shr ax, 14
                            ; and eax, 1
                            ; xor eax, 1
                            ; mov Rd(left as u8), eax
                        );
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64Eqz => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; cmp Rq(reg as u8), 0
                            ; lahf
                            ; shr ax, 14
                            ; and eax, 1
                        );
                        if reg != Register::RAX {
                            dynasm!(
                                assembler
                                ; mov Rd(reg as u8), eax
                            );
                        }
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64Clz => {
                Self::emit_unop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; lzcnt Rq(reg as u8), Rq(reg as u8)
                        );
                    },
                )?;
            }
            Operator::I64Ctz => {
                Self::emit_unop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; tzcnt Rq(reg as u8), Rq(reg as u8)
                        );
                    },
                )?;
            }
            Operator::I64Popcnt => {
                Self::emit_unop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; popcnt Rq(reg as u8), Rq(reg as u8)
                        );
                    },
                )?;
            }
            Operator::I64Shl => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; shl Rq(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I64ShrU => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; shr Rq(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I64ShrS => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; sar Rq(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I64Rotl => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; rol Rq(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            Operator::I64Rotr => {
                Self::emit_binop_i64(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
                        Self::emit_shift(assembler, value_stack, left, right, |assembler, left| {
                            dynasm!(
                                assembler
                                ; ror Rq(left as u8), cl
                            )
                        });
                    },
                )?;
            }
            // Comparison operators.
            // https://en.wikibooks.org/wiki/X86_Assembly/Control_Flow
            // TODO: Is reading flag register directly faster?
            Operator::I64LtS => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jl >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64LeS => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jle >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64GtS => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jg >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64GeS => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jge >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64LtU => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jb >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64LeU => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jbe >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64GtU => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; ja >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64GeU => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        Self::emit_cmp_i64(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jae >label_true
                            );
                        });
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::I64ExtendSI32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movsx Rq(reg as u8), Rd(reg as u8)
                        );
                    },
                    WpType::I32,
                    WpType::I64,
                )?;
            }
            Operator::I64ExtendUI32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |_assembler, _value_stack, _reg| {
                        // FIXME: Is it correct to do nothing here?
                    },
                    WpType::I32,
                    WpType::I64,
                )?;
            }
            Operator::I32WrapI64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), Rd(reg as u8) // clear upper 32 bits
                        );
                    },
                    WpType::I64,
                    WpType::I32,
                )?;
            }
            Operator::Block { ty } => {
                self.control_stack
                    .as_mut()
                    .unwrap()
                    .frames
                    .push(ControlFrame {
                        label: assembler.new_dynamic_label(),
                        loop_like: false,
                        if_else: IfElseState::None,
                        returns: match ty {
                            WpType::EmptyBlockType => vec![],
                            _ => vec![ty],
                        },
                        value_stack_depth_before: self.value_stack.values.len(),
                    });
            }
            Operator::Unreachable => {
                dynasm!(
                    assembler
                    ; ud2
                );
                self.unreachable_depth = 1;
            }
            Operator::Drop => {
                let info = self.value_stack.pop()?;
                Self::gen_rt_pop(assembler, &info)?;
            }
            Operator::Return => {
                Self::emit_return(assembler, &mut self.value_stack, &self.returns)?;
                self.unreachable_depth = 1;
            }
            Operator::Call { function_index } => {
                let function_index = function_index as usize;
                let label = self
                    .function_labels
                    .as_mut()
                    .unwrap()
                    .entry(function_index)
                    .or_insert_with(|| (assembler.new_dynamic_label(), None))
                    .0;
                let sig_index = match self.function_signatures.get(FuncIndex::new(function_index)) {
                    Some(x) => *x,
                    None => {
                        return Err(CodegenError {
                            message: "signature not found",
                        });
                    }
                };
                let sig = match self.signatures.get(sig_index) {
                    Some(x) => x,
                    None => {
                        return Err(CodegenError {
                            message: "signature does not exist",
                        });
                    }
                };
                let param_types: Vec<WpType> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: Vec<WpType> =
                    sig.returns().iter().cloned().map(type_to_wp_type).collect();
                Self::emit_call_raw(
                    assembler,
                    &mut self.value_stack,
                    label,
                    &param_types,
                    &return_types,
                )?;
            }
            Operator::CallIndirect { index, table_index } => {
                if table_index != 0 {
                    return Err(CodegenError {
                        message: "only one table is supported",
                    });
                }
                let local_or_import = if module_info.tables.len() > 0 {
                    if module_info.tables.len() != 1 || module_info.imported_tables.len() != 0 {
                        return Err(CodegenError {
                            message: "only one table is supported",
                        });
                    }
                    CallIndirectLocalOrImport::Local
                } else if module_info.imported_tables.len() > 0 {
                    if module_info.tables.len() != 0 || module_info.imported_tables.len() != 1 {
                        return Err(CodegenError {
                            message: "only one table is supported",
                        });
                    }
                    CallIndirectLocalOrImport::Import
                } else {
                    return Err(CodegenError {
                        message: "no tables",
                    });
                };
                let sig_index = SigIndex::new(index as usize);
                let sig = match self.signatures.get(sig_index) {
                    Some(x) => x,
                    None => {
                        return Err(CodegenError {
                            message: "signature does not exist",
                        });
                    }
                };
                let mut param_types: Vec<WpType> =
                    sig.params().iter().cloned().map(type_to_wp_type).collect();
                let return_types: Vec<WpType> =
                    sig.returns().iter().cloned().map(type_to_wp_type).collect();
                param_types.push(WpType::I32); // element index

                dynasm!(
                    assembler
                    ; jmp >after_trampoline
                );

                let trampoline_label = Self::emit_native_call_trampoline(
                    assembler,
                    call_indirect,
                    index as usize,
                    local_or_import,
                );

                dynasm!(
                    assembler
                    ; after_trampoline:
                );

                Self::emit_call_raw(
                    assembler,
                    &mut self.value_stack,
                    trampoline_label,
                    &param_types,
                    &return_types,
                )?;
            }
            Operator::End => {
                if self.control_stack.as_ref().unwrap().frames.len() == 1 {
                    let frame = self.control_stack.as_mut().unwrap().frames.pop().unwrap();

                    if !was_unreachable {
                        Self::emit_leave_frame(assembler, &frame, &mut self.value_stack, false)?;
                    } else {
                        self.value_stack.reset_depth(0);
                    }

                    dynasm!(
                        assembler
                        ; =>frame.label
                    );
                } else {
                    Self::emit_block_end(
                        assembler,
                        self.control_stack.as_mut().unwrap(),
                        &mut self.value_stack,
                        was_unreachable,
                    )?;
                }
            }
            Operator::Loop { ty } => {
                let label = assembler.new_dynamic_label();
                self.control_stack
                    .as_mut()
                    .unwrap()
                    .frames
                    .push(ControlFrame {
                        label: label,
                        loop_like: true,
                        if_else: IfElseState::None,
                        returns: match ty {
                            WpType::EmptyBlockType => vec![],
                            _ => vec![ty],
                        },
                        value_stack_depth_before: self.value_stack.values.len(),
                    });
                dynasm!(
                    assembler
                    ; =>label
                );
            }
            Operator::If { ty } => {
                let label_end = assembler.new_dynamic_label();
                let label_else = assembler.new_dynamic_label();

                Self::emit_pop_into_ax(assembler, &mut self.value_stack)?; // TODO: typeck?

                self.control_stack
                    .as_mut()
                    .unwrap()
                    .frames
                    .push(ControlFrame {
                        label: label_end,
                        loop_like: false,
                        if_else: IfElseState::If(label_else),
                        returns: match ty {
                            WpType::EmptyBlockType => vec![],
                            _ => vec![ty],
                        },
                        value_stack_depth_before: self.value_stack.values.len(),
                    });
                dynasm!(
                    assembler
                    ; cmp eax, 0
                    ; je =>label_else
                );
            }
            Operator::Else => {
                Self::emit_else(
                    assembler,
                    self.control_stack.as_mut().unwrap(),
                    &mut self.value_stack,
                    was_unreachable,
                )?;
            }
            Operator::Select => {
                Self::emit_pop_into_ax(assembler, &mut self.value_stack)?;
                let v_b = self.value_stack.pop()?;
                let v_a = self.value_stack.pop()?;

                if v_b.ty != v_a.ty {
                    return Err(CodegenError {
                        message: "select: type mismatch",
                    });
                }

                dynasm!(
                    assembler
                    ; cmp eax, 0
                );
                match v_b.location {
                    ValueLocation::Stack => {
                        dynasm!(
                            assembler
                            ; cmove rax, [rsp]
                            ; add rsp, 8
                        );
                    }
                    ValueLocation::Register(x) => {
                        let reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; cmove rax, Rq(reg as u8)
                        );
                    }
                }
                match v_a.location {
                    ValueLocation::Stack => {
                        dynasm!(
                            assembler
                            ; cmovne rax, [rsp]
                            ; add rsp, 8
                        );
                    }
                    ValueLocation::Register(x) => {
                        let reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; cmovne rax, Rq(reg as u8)
                        );
                    }
                }

                Self::emit_push_from_ax(assembler, &mut self.value_stack, v_a.ty)?;
            }
            Operator::Br { relative_depth } => {
                Self::emit_jmp(
                    assembler,
                    self.control_stack.as_ref().unwrap(),
                    &mut self.value_stack,
                    relative_depth as usize,
                )?;
                self.unreachable_depth = 1;
            }
            Operator::BrIf { relative_depth } => {
                let no_br_label = assembler.new_dynamic_label();
                Self::emit_pop_into_ax(assembler, &mut self.value_stack)?; // TODO: typeck?
                dynasm!(
                    assembler
                    ; cmp eax, 0
                    ; je =>no_br_label
                );
                Self::emit_jmp(
                    assembler,
                    self.control_stack.as_ref().unwrap(),
                    &mut self.value_stack,
                    relative_depth as usize,
                )?;
                dynasm!(
                    assembler
                    ; =>no_br_label
                );
            }
            Operator::BrTable { table } => {
                let (targets, default_target) = match table.read_table() {
                    Ok(x) => x,
                    Err(_) => {
                        return Err(CodegenError {
                            message: "cannot read br table",
                        });
                    }
                };
                let cond_ty = Self::emit_pop_into_ax(assembler, &mut self.value_stack)?;
                if cond_ty != WpType::I32 {
                    return Err(CodegenError {
                        message: "expecting i32 for BrTable condition",
                    });
                }
                let mut table = vec![0usize; targets.len()];
                dynasm!(
                    assembler
                    ; cmp eax, targets.len() as i32
                    ; jae >default_br
                    ; shl rax, 3
                    ; push rcx
                    ; mov rcx, QWORD table.as_ptr() as usize as i64
                    ; add rax, rcx
                    ; pop rcx
                    ; mov rax, [rax] // assuming upper 32 bits of rax are zeroed
                    ; jmp rax
                );
                for (i, target) in targets.iter().enumerate() {
                    let AssemblyOffset(offset) = assembler.offset();
                    table[i] = offset;
                    Self::emit_jmp(
                        assembler,
                        self.control_stack.as_ref().unwrap(),
                        &mut self.value_stack,
                        *target as usize,
                    )?; // This does not actually modify value_stack.
                }
                dynasm!(
                    assembler
                    ; default_br:
                );
                Self::emit_jmp(
                    assembler,
                    self.control_stack.as_ref().unwrap(),
                    &mut self.value_stack,
                    default_target as usize,
                )?;
                self.br_table_data.as_mut().unwrap().push(table);
                self.unreachable_depth = 1;
            }
            Operator::I32Load { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I32,
                    module_info,
                    4,
                )?;
            }
            Operator::I32Load8U { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movzx Rd(reg as u8), BYTE [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I32,
                    module_info,
                    1,
                )?;
            }
            Operator::I32Load8S { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movsx Rd(reg as u8), BYTE [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I32,
                    module_info,
                    1,
                )?;
            }
            Operator::I32Load16U { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movzx Rd(reg as u8), WORD [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I32,
                    module_info,
                    2,
                )?;
            }
            Operator::I32Load16S { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movsx Rd(reg as u8), WORD [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I32,
                    module_info,
                    2,
                )?;
            }
            Operator::I32Store { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rd(value_reg as u8)
                        );
                    },
                    WpType::I32,
                    module_info,
                    4,
                )?;
            }
            Operator::I32Store8 { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rb(value_reg as u8)
                        );
                    },
                    WpType::I32,
                    module_info,
                    1,
                )?;
            }
            Operator::I32Store16 { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rw(value_reg as u8)
                        );
                    },
                    WpType::I32,
                    module_info,
                    2,
                )?;
            }
            Operator::I64Load { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; mov Rq(reg as u8), [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I64,
                    module_info,
                    8,
                )?;
            }
            Operator::I64Load8U { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movzx Rq(reg as u8), BYTE [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I64,
                    module_info,
                    1,
                )?;
            }
            Operator::I64Load8S { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movsx Rq(reg as u8), BYTE [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I64,
                    module_info,
                    1,
                )?;
            }
            Operator::I64Load16U { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movzx Rq(reg as u8), WORD [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I64,
                    module_info,
                    2,
                )?;
            }
            Operator::I64Load16S { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movsx Rq(reg as u8), WORD [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I64,
                    module_info,
                    2,
                )?;
            }
            Operator::I64Load32U { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), DWORD [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I64,
                    module_info,
                    4,
                )?;
            }
            Operator::I64Load32S { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; movsx Rq(reg as u8), DWORD [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::I64,
                    module_info,
                    4,
                )?;
            }
            Operator::I64Store { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rq(value_reg as u8)
                        );
                    },
                    WpType::I64,
                    module_info,
                    8,
                )?;
            }
            Operator::I64Store8 { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rb(value_reg as u8)
                        );
                    },
                    WpType::I64,
                    module_info,
                    1,
                )?;
            }
            Operator::I64Store16 { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rw(value_reg as u8)
                        );
                    },
                    WpType::I64,
                    module_info,
                    2,
                )?;
            }
            Operator::I64Store32 { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rd(value_reg as u8)
                        );
                    },
                    WpType::I64,
                    module_info,
                    4,
                )?;
            }
            Operator::F32Const { value } => {
                let location = self.value_stack.push(WpType::F32);
                match location {
                    ValueLocation::Register(x) => {
                        let reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), value.bits() as i32
                        );
                    }
                    ValueLocation::Stack => {
                        dynasm!(
                            assembler
                            ; push value.bits() as i32
                        );
                    }
                }
            }
            Operator::F64Const { value } => {
                let location = self.value_stack.push(WpType::F64);
                match location {
                    ValueLocation::Register(x) => {
                        let reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; mov Rq(reg as u8), QWORD value.bits() as i64
                        );
                    }
                    ValueLocation::Stack => {
                        dynasm!(
                            assembler
                            ; mov rax, QWORD value.bits() as i64
                            ; push rax
                        );
                    }
                }
            }
            Operator::F32Load { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::F32,
                    module_info,
                    4,
                )?;
            }
            Operator::F32Store { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rd(value_reg as u8)
                        );
                    },
                    WpType::F32,
                    module_info,
                    4,
                )?;
            }
            Operator::F64Load { memarg } => {
                Self::emit_memory_load(
                    assembler,
                    &mut self.value_stack,
                    |assembler, reg| {
                        dynasm!(
                            assembler
                            ; mov Rq(reg as u8), [Rq(reg as u8) + memarg.offset as i32]
                        );
                    },
                    WpType::F64,
                    module_info,
                    8,
                )?;
            }
            Operator::F64Store { memarg } => {
                Self::emit_memory_store(
                    assembler,
                    &mut self.value_stack,
                    |assembler, addr_reg, value_reg| {
                        dynasm!(
                            assembler
                            ; mov [Rq(addr_reg as u8) + memarg.offset as i32], Rq(value_reg as u8)
                        );
                    },
                    WpType::F64,
                    module_info,
                    8,
                )?;
            }
            Operator::I32ReinterpretF32 => {
                Self::emit_reinterpret(&mut self.value_stack, WpType::F32, WpType::I32)?;
            }
            Operator::F32ReinterpretI32 => {
                Self::emit_reinterpret(&mut self.value_stack, WpType::I32, WpType::F32)?;
            }
            Operator::I64ReinterpretF64 => {
                Self::emit_reinterpret(&mut self.value_stack, WpType::F64, WpType::I64)?;
            }
            Operator::F64ReinterpretI64 => {
                Self::emit_reinterpret(&mut self.value_stack, WpType::I64, WpType::F64)?;
            }
            Operator::F32ConvertSI32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; cvtsi2ss xmm1, Rd(reg as u8)
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::I32,
                    WpType::F32,
                )?;
            }
            Operator::F32ConvertUI32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), Rd(reg as u8) // clear upper 32 bits
                            ; cvtsi2ss xmm1, Rq(reg as u8)
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::I32,
                    WpType::F32,
                )?;
            }
            Operator::F32ConvertSI64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; cvtsi2ss xmm1, Rq(reg as u8)
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::I64,
                    WpType::F32,
                )?;
            }
            /*
                0:   48 85 ff                test   %rdi,%rdi
                3:   78 0b                   js     10 <ulong2double+0x10>
                5:   c4 e1 fb 2a c7          vcvtsi2sd %rdi,%xmm0,%xmm0
                a:   c3                      retq
                b:   0f 1f 44 00 00          nopl   0x0(%rax,%rax,1)
                10:   48 89 f8                mov    %rdi,%rax
                13:   83 e7 01                and    $0x1,%edi
                16:   48 d1 e8                shr    %rax
                19:   48 09 f8                or     %rdi,%rax
                1c:   c4 e1 fb 2a c0          vcvtsi2sd %rax,%xmm0,%xmm0
                21:   c5 fb 58 c0             vaddsd %xmm0,%xmm0,%xmm0
                25:   c3                      retq
            */
            Operator::F32ConvertUI64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; test Rq(reg as u8), Rq(reg as u8)
                            ; js >do_convert
                            ; cvtsi2ss xmm1, Rq(reg as u8)
                            ; movd Rd(reg as u8), xmm1
                            ; jmp >end_convert
                            ; do_convert:
                            ; movq xmm5, r15
                            ; mov r15, Rq(reg as u8)
                            ; and r15, 1
                            ; shr Rq(reg as u8), 1
                            ; or Rq(reg as u8), r15
                            ; cvtsi2ss xmm1, Rq(reg as u8)
                            ; addss xmm1, xmm1
                            ; movq r15, xmm5
                            ; movd Rd(reg as u8), xmm1
                            ; end_convert:
                        );
                    },
                    WpType::I64,
                    WpType::F32,
                )?;
            }
            Operator::F64ConvertSI32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; cvtsi2sd xmm1, Rd(reg as u8)
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::I32,
                    WpType::F64,
                )?;
            }
            Operator::F64ConvertUI32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; mov Rd(reg as u8), Rd(reg as u8) // clear upper 32 bits
                            ; cvtsi2sd xmm1, Rq(reg as u8)
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::I32,
                    WpType::F64,
                )?;
            }
            Operator::F64ConvertSI64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; cvtsi2sd xmm1, Rq(reg as u8)
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::I64,
                    WpType::F64,
                )?;
            }
            Operator::F64ConvertUI64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; test Rq(reg as u8), Rq(reg as u8)
                            ; js >do_convert
                            ; cvtsi2sd xmm1, Rq(reg as u8)
                            ; movq Rq(reg as u8), xmm1
                            ; jmp >end_convert
                            ; do_convert:
                            ; movq xmm5, r15
                            ; mov r15, Rq(reg as u8)
                            ; and r15, 1
                            ; shr Rq(reg as u8), 1
                            ; or Rq(reg as u8), r15
                            ; cvtsi2sd xmm1, Rq(reg as u8)
                            ; addsd xmm1, xmm1
                            ; movq r15, xmm5
                            ; movq Rq(reg as u8), xmm1
                            ; end_convert:
                        );
                    },
                    WpType::I64,
                    WpType::F64,
                )?;
            }
            Operator::F64PromoteF32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; cvtss2sd xmm1, xmm1
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F64,
                )?;
            }
            Operator::F32DemoteF64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; cvtsd2ss xmm1, xmm1
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F32,
                )?;
            }
            Operator::F32Add => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; addss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Sub => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; subss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Mul => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; mulss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Div => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; divss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Max => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; maxss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Min => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; minss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Eq => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; cmpeqss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::F32Ne => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; cmpneqss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::F32Gt => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; vcmpgtss xmm1, xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::F32Ge => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; vcmpgess xmm1, xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::F32Lt => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; cmpltss xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::F32Le => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; cmpless xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::F32Copysign => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(left as u8)
                            ; movd xmm2, Rd(right as u8)
                            ; mov eax, 0x7fffffffu32 as i32
                            ; movd xmm3, eax
                            ; pand xmm1, xmm3
                            ; mov eax, 0x80000000u32 as i32
                            ; movd xmm3, eax
                            ; pand xmm2, xmm3
                            ; por xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Sqrt => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; sqrtss xmm1, xmm1
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Abs => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; and Rd(reg as u8), 0x7fffffffu32 as i32
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Neg => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; btc Rd(reg as u8), 31
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Nearest => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; roundss xmm1, xmm1, 0
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Floor => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; roundss xmm1, xmm1, 1
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Ceil => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; roundss xmm1, xmm1, 2
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::F32Trunc => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; roundss xmm1, xmm1, 3
                            ; movd Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::F32,
                )?;
            }
            Operator::I32TruncUF32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f32_int_conv_check(assembler, reg, -1.0, 4294967296.0);
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; cvttss2si Rq(reg as u8), xmm1
                            ; mov Rd(reg as u8), Rd(reg as u8)
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::I32TruncSF32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f32_int_conv_check(assembler, reg, -2147483904.0, 2147483648.0);
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; cvttss2si Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::I32,
                )?;
            }
            Operator::I64TruncUF32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f32_int_conv_check(assembler, reg, -1.0, 18446744073709551616.0);
                        /*
                            LCPI0_0:
                                .long   1593835520              ## float 9.22337203E+18

                            movss   LCPI0_0(%rip), %xmm1    ## xmm1 = mem[0],zero,zero,zero
                            movaps  %xmm0, %xmm2
                            subss   %xmm1, %xmm2
                            cvttss2si       %xmm2, %rax
                            movabsq $-9223372036854775808, %rcx ## imm = 0x8000000000000000
                            xorq    %rax, %rcx
                            cvttss2si       %xmm0, %rax
                            ucomiss %xmm1, %xmm0
                            cmovaeq %rcx, %rax
                        */
                        dynasm!(
                            assembler
                            ; movq xmm5, r15
                            ; mov r15d, 1593835520u32 as i32 //float 9.22337203E+18
                            ; movd xmm1, r15d
                            ; movd xmm2, Rd(reg as u8)
                            ; movd xmm3, Rd(reg as u8)
                            ; subss xmm2, xmm1
                            ; cvttss2si Rq(reg as u8), xmm2
                            ; mov r15, QWORD 0x8000000000000000u64 as i64
                            ; xor r15, Rq(reg as u8)
                            ; cvttss2si Rq(reg as u8), xmm3
                            ; ucomiss xmm3, xmm1
                            ; cmovae Rq(reg as u8), r15
                            ; movq r15, xmm5
                        );
                    },
                    WpType::F32,
                    WpType::I64,
                )?;
            }
            Operator::I64TruncSF32 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f32_int_conv_check(
                            assembler,
                            reg,
                            -9223373136366403584.0,
                            9223372036854775808.0,
                        );
                        dynasm!(
                            assembler
                            ; movd xmm1, Rd(reg as u8)
                            ; cvttss2si Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F32,
                    WpType::I64,
                )?;
            }
            Operator::F64Add => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; addsd xmm1, xmm2
                            ; movq Rq(left as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Sub => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; subsd xmm1, xmm2
                            ; movq Rq(left as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Mul => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; mulsd xmm1, xmm2
                            ; movq Rq(left as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Div => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; divsd xmm1, xmm2
                            ; movq Rq(left as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Max => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; maxsd xmm1, xmm2
                            ; movq Rq(left as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Min => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; minsd xmm1, xmm2
                            ; movq Rq(left as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Eq => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; cmpeqsd xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::F64Ne => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; cmpneqsd xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::F64Gt => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; vcmpgtsd xmm1, xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::F64Ge => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; vcmpgesd xmm1, xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::F64Lt => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; cmpltsd xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::F64Le => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; cmplesd xmm1, xmm2
                            ; movd Rd(left as u8), xmm1
                            ; and Rd(left as u8), 1
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::F64Copysign => {
                Self::emit_binop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(left as u8)
                            ; movq xmm2, Rq(right as u8)
                            ; mov rax, QWORD 0x7fffffffffffffffu64 as i64
                            ; movq xmm3, rax
                            ; pand xmm1, xmm3
                            ; mov rax, QWORD 0x8000000000000000u64 as i64
                            ; movq xmm3, rax
                            ; pand xmm2, xmm3
                            ; por xmm1, xmm2
                            ; movq Rq(left as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Sqrt => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; sqrtsd xmm1, xmm1
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Abs => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; mov rax, QWORD 0x7fffffffffffffff
                            ; movq xmm2, rax
                            ; pand xmm1, xmm2
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Neg => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; btc Rq(reg as u8), 63
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Nearest => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; roundsd xmm1, xmm1, 0
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Floor => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; roundsd xmm1, xmm1, 1
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Ceil => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; roundsd xmm1, xmm1, 2
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::F64Trunc => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; roundsd xmm1, xmm1, 3
                            ; movq Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::F64,
                )?;
            }
            Operator::I32TruncUF64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f64_int_conv_check(assembler, reg, -1.0, 4294967296.0);

                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; cvttsd2si Rq(reg as u8), xmm1
                            ; mov Rd(reg as u8), Rd(reg as u8)
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::I32TruncSF64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f64_int_conv_check(assembler, reg, -2147483649.0, 2147483648.0);

                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; cvttsd2si Rd(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::I32,
                )?;
            }
            Operator::I64TruncUF64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f64_int_conv_check(assembler, reg, -1.0, 18446744073709551616.0);

                        /*
                            LCPI0_0:
                                .quad   4890909195324358656     ## double 9.2233720368547758E+18

                            movsd   LCPI0_0(%rip), %xmm1    ## xmm1 = mem[0],zero
                            movapd  %xmm0, %xmm2
                            subsd   %xmm1, %xmm2
                            cvttsd2si       %xmm2, %rax
                            movabsq $-9223372036854775808, %rcx ## imm = 0x8000000000000000
                            xorq    %rax, %rcx
                            cvttsd2si       %xmm0, %rax
                            ucomisd %xmm1, %xmm0
                            cmovaeq %rcx, %rax
                        */

                        dynasm!(
                            assembler
                            ; movq xmm5, r15
                            ; mov r15, QWORD 4890909195324358656u64 as i64 //double 9.2233720368547758E+18
                            ; movq xmm1, r15
                            ; movq xmm2, Rq(reg as u8)
                            ; movq xmm3, Rq(reg as u8)
                            ; subsd xmm2, xmm1
                            ; cvttsd2si Rq(reg as u8), xmm2
                            ; mov r15, QWORD 0x8000000000000000u64 as i64
                            ; xor r15, Rq(reg as u8)
                            ; cvttsd2si Rq(reg as u8), xmm3
                            ; ucomisd xmm3, xmm1
                            ; cmovae Rq(reg as u8), r15
                            ; movq r15, xmm5
                        );
                    },
                    WpType::F64,
                    WpType::I64,
                )?;
            }
            Operator::I64TruncSF64 => {
                Self::emit_unop(
                    assembler,
                    &mut self.value_stack,
                    |assembler, _value_stack, reg| {
                        Self::emit_f64_int_conv_check(
                            assembler,
                            reg,
                            -9223372036854777856.0,
                            9223372036854775808.0,
                        );

                        dynasm!(
                            assembler
                            ; movq xmm1, Rq(reg as u8)
                            ; cvttsd2si Rq(reg as u8), xmm1
                        );
                    },
                    WpType::F64,
                    WpType::I64,
                )?;
            }
            Operator::Nop => {}
            Operator::MemorySize { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                let label = match memory_index.local_or_import(module_info) {
                    LocalOrImport::Local(local_mem_index) => {
                        let mem_desc = &module_info.memories[local_mem_index];
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => {
                                self.native_trampolines.memory_size_dynamic_local
                            }
                            MemoryType::Static => self.native_trampolines.memory_size_static_local,
                            MemoryType::SharedStatic => {
                                self.native_trampolines.memory_size_shared_local
                            }
                        }
                    }
                    LocalOrImport::Import(import_mem_index) => {
                        let mem_desc = &module_info.imported_memories[import_mem_index].1;
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => {
                                self.native_trampolines.memory_size_dynamic_import
                            }
                            MemoryType::Static => self.native_trampolines.memory_size_static_import,
                            MemoryType::SharedStatic => {
                                self.native_trampolines.memory_size_shared_import
                            }
                        }
                    }
                };
                Self::emit_call_raw(assembler, &mut self.value_stack, label, &[], &[WpType::I32])?;
            }
            Operator::MemoryGrow { reserved } => {
                let memory_index = MemoryIndex::new(reserved as usize);
                let label = match memory_index.local_or_import(module_info) {
                    LocalOrImport::Local(local_mem_index) => {
                        let mem_desc = &module_info.memories[local_mem_index];
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => {
                                self.native_trampolines.memory_grow_dynamic_local
                            }
                            MemoryType::Static => self.native_trampolines.memory_grow_static_local,
                            MemoryType::SharedStatic => {
                                self.native_trampolines.memory_grow_shared_local
                            }
                        }
                    }
                    LocalOrImport::Import(import_mem_index) => {
                        let mem_desc = &module_info.imported_memories[import_mem_index].1;
                        match mem_desc.memory_type() {
                            MemoryType::Dynamic => {
                                self.native_trampolines.memory_grow_dynamic_import
                            }
                            MemoryType::Static => self.native_trampolines.memory_grow_static_import,
                            MemoryType::SharedStatic => {
                                self.native_trampolines.memory_grow_shared_import
                            }
                        }
                    }
                };
                Self::emit_call_raw(
                    assembler,
                    &mut self.value_stack,
                    label,
                    &[WpType::I32],
                    &[WpType::I32],
                )?;
                Self::emit_update_memory_from_ctx(assembler, module_info)?;
            }
            _ => {
                panic!("{:?}", op);
            }
        }
        Ok(())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        let assembler = self.assembler.as_mut().unwrap();

        dynasm!(
            assembler
            ; mov rsp, rbp
            ; pop rbp
            ; ret
        );

        if self.value_stack.values.len() != 0
            || self.control_stack.as_ref().unwrap().frames.len() != 0
        {
            return Err(CodegenError {
                message: "control/value stack not empty at end of function",
            });
        }

        Ok(())
    }
}

fn get_size_of_type(ty: &WpType) -> Result<usize, CodegenError> {
    match *ty {
        WpType::I32 | WpType::F32 => Ok(4),
        WpType::I64 | WpType::F64 => Ok(8),
        _ => Err(CodegenError {
            message: "unknown type",
        }),
    }
}

fn is_dword(n: usize) -> bool {
    n == 4
}

fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
    }
}

unsafe extern "C" fn invoke_import(
    _unused: usize,
    import_id: usize,
    stack_top: *mut u8,
    stack_base: *mut u8,
    _vmctx: *mut vm::Ctx,
    _memory_base: *mut u8,
) -> u64 {
    let vmctx: &mut vm::InternalCtx = &mut *(_vmctx as *mut vm::InternalCtx);
    let import = (*vmctx.imported_funcs.offset(import_id as isize)).func;

    CONSTRUCT_STACK_AND_CALL_NATIVE(stack_top, stack_base, _vmctx, import)
}

#[repr(u64)]
#[derive(Copy, Clone, Debug)]
enum CallIndirectLocalOrImport {
    Local,
    Import,
}

#[allow(clippy::cast_ptr_alignment)]
unsafe extern "C" fn call_indirect(
    sig_index: usize,
    local_or_import: CallIndirectLocalOrImport,
    mut stack_top: *mut u8,
    stack_base: *mut u8,
    vmctx: *mut vm::Ctx,
    _memory_base: *mut u8,
) -> u64 {
    let elem_index = *(stack_top as *mut u32) as usize;
    stack_top = stack_top.offset(8);
    assert!(stack_top as usize <= stack_base as usize);

    let table: &LocalTable = match local_or_import {
        CallIndirectLocalOrImport::Local => &*(*(*(vmctx as *mut vm::InternalCtx)).tables),
        CallIndirectLocalOrImport::Import => {
            &*(*(*(vmctx as *mut vm::InternalCtx)).imported_tables)
        }
    };
    if elem_index >= table.count as usize {
        eprintln!("element index out of bounds");
        protect_unix::trigger_trap();
    }
    let anyfunc = &*(table.base as *mut vm::Anyfunc).offset(elem_index as isize);
    let dynamic_sigindex = *(*(vmctx as *mut vm::InternalCtx))
        .dynamic_sigindices
        .offset(sig_index as isize);

    if anyfunc.func.is_null() {
        eprintln!("null anyfunc");
        protect_unix::trigger_trap();
    }

    if anyfunc.sig_id.0 != dynamic_sigindex.0 {
        eprintln!("signature mismatch");
        protect_unix::trigger_trap();
    }

    CONSTRUCT_STACK_AND_CALL_NATIVE(stack_top, stack_base, anyfunc.ctx, anyfunc.func)
}

#[repr(u64)]
#[derive(Copy, Clone, Debug)]
enum MemoryKind {
    DynamicLocal,
    StaticLocal,
    SharedLocal,
    DynamicImport,
    StaticImport,
    SharedImport,
}

unsafe extern "C" fn _memory_size(
    op: MemoryKind,
    index: usize,
    _stack_top: *mut u8,
    _stack_base: *mut u8,
    vmctx: *mut vm::Ctx,
    _memory_base: *mut u8,
) -> u64 {
    use wasmer_runtime_core::vmcalls;
    let ret = match op {
        MemoryKind::DynamicLocal => {
            vmcalls::local_dynamic_memory_size(&*vmctx, LocalMemoryIndex::new(index))
        }
        MemoryKind::StaticLocal => {
            vmcalls::local_static_memory_size(&*vmctx, LocalMemoryIndex::new(index))
        }
        MemoryKind::SharedLocal => unreachable!(),
        MemoryKind::DynamicImport => {
            vmcalls::imported_dynamic_memory_size(&*vmctx, ImportedMemoryIndex::new(index))
        }
        MemoryKind::StaticImport => {
            vmcalls::imported_static_memory_size(&*vmctx, ImportedMemoryIndex::new(index))
        }
        MemoryKind::SharedImport => unreachable!(),
    };
    ret.0 as u32 as u64
}

#[allow(clippy::cast_ptr_alignment)]
unsafe extern "C" fn _memory_grow(
    op: MemoryKind,
    index: usize,
    stack_top: *mut u8,
    stack_base: *mut u8,
    vmctx: *mut vm::Ctx,
    _memory_base: *mut u8,
) -> u64 {
    use wasmer_runtime_core::vmcalls;
    assert_eq!(stack_base as usize - stack_top as usize, 8);
    let pages = Pages(*(stack_top as *mut u32));
    let ret = match op {
        MemoryKind::DynamicLocal => {
            vmcalls::local_dynamic_memory_grow(&mut *vmctx, LocalMemoryIndex::new(index), pages)
        }
        MemoryKind::StaticLocal => {
            vmcalls::local_static_memory_grow(&mut *vmctx, LocalMemoryIndex::new(index), pages)
        }
        MemoryKind::SharedLocal => unreachable!(),
        MemoryKind::DynamicImport => vmcalls::imported_dynamic_memory_grow(
            &mut *vmctx,
            ImportedMemoryIndex::new(index),
            pages,
        ),
        MemoryKind::StaticImport => vmcalls::imported_static_memory_grow(
            &mut *vmctx,
            ImportedMemoryIndex::new(index),
            pages,
        ),
        MemoryKind::SharedImport => unreachable!(),
    };
    ret as u32 as u64
}
