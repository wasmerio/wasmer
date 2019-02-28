use super::codegen::*;
use super::stack::{ControlFrame, ControlStack, IfElseState, ValueInfo, ValueLocation, ValueStack};
use byteorder::{ByteOrder, LittleEndian};
use dynasmrt::{
    x64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, ExecutableBuffer,
};
use std::{collections::HashMap, sync::Arc};
use wasmer_runtime_core::{
    backend::{Backend, Compiler, FuncResolver, ProtectedCaller, Token, UserTrapper},
    error::{CompileError, CompileResult, RuntimeError, RuntimeResult},
    module::{ModuleInfo, ModuleInner, StringTable},
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, GlobalIndex, LocalFuncIndex, MemoryIndex, SigIndex, TableIndex, Type,
        Value,
    },
    vm::{self, ImportBacking},
};
use wasmparser::{Operator, Type as WpType};

lazy_static! {
    static ref CALL_WASM: unsafe extern "C" fn(params: *const u8, params_len: usize, target: *const u8) -> i64 = {
        let mut assembler = Assembler::new().unwrap();
        let offset = assembler.offset();
        dynasm!(
            assembler
            ; push rbx
            ; push r12
            ; push r13
            ; push r14
            ; push r15
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
            ; mov eax, [rdi]
            ; mov [r8], eax
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
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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
    pub fn from_scratch_reg(id: u8) -> Register {
        use self::Register::*;
        match id {
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
            10 => R13,
            11 => R14,
            // 12 => R15, // R15 is reserved for memory base pointer.
            _ => unreachable!(),
        }
    }

    pub fn is_used(&self, stack: &ValueStack) -> bool {
        use self::Register::*;
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

#[repr(u64)]
#[derive(Copy, Clone, Debug)]
pub enum TrapCode {
    Unreachable,
}

pub struct NativeTrampolines {
    trap_unreachable: DynamicLabel,
}

pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
    signatures: Option<Arc<Map<SigIndex, Arc<FuncSig>>>>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    assembler: Option<Assembler>,
    native_trampolines: Arc<NativeTrampolines>,
}

pub struct X64FunctionCode {
    signatures: Arc<Map<SigIndex, Arc<FuncSig>>>,
    function_signatures: Arc<Map<FuncIndex, SigIndex>>,
    native_trampolines: Arc<NativeTrampolines>,

    id: usize,
    begin_label: DynamicLabel,
    begin_offset: AssemblyOffset,
    assembler: Option<Assembler>,
    function_labels: Option<HashMap<usize, DynamicLabel>>,
    br_table_data: Option<Vec<Vec<usize>>>,
    returns: Vec<WpType>,
    locals: Vec<Local>,
    num_params: usize,
    current_stack_offset: usize,
    value_stack: ValueStack,
    control_stack: Option<ControlStack>,
    unreachable_depth: usize,
}

pub struct X64ExecutionContext {
    code: ExecutableBuffer,
    functions: Vec<X64FunctionCode>,
    br_table_data: Vec<Vec<usize>>,
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
        let index = _func_index.index();
        let ptr = self.code.ptr(self.functions[index].begin_offset);
        let return_ty = self.functions[index].returns.last().cloned();

        if self.functions[index].num_params != _params.len() {
            return Err(RuntimeError::User {
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
                        return Err(RuntimeError::User {
                            msg: "signature mismatch".into(),
                        })
                    }
                }
            } else {
                match _params[i] {
                    Value::I64(x) => LittleEndian::write_u64(buf, x as u64),
                    Value::F64(x) => LittleEndian::write_u64(buf, f64::to_bits(x)),
                    _ => {
                        return Err(RuntimeError::User {
                            msg: "signature mismatch".into(),
                        })
                    }
                }
            }
        }

        let ret = unsafe { CALL_WASM(param_buf.as_ptr(), param_buf.len(), ptr) };
        Ok(if let Some(ty) = return_ty {
            vec![Value::I64(ret)]
        } else {
            vec![]
        })
    }

    fn get_early_trapper(&self) -> Box<dyn UserTrapper> {
        pub struct Trapper;

        impl UserTrapper for Trapper {
            unsafe fn do_early_trap(&self, msg: String) -> ! {
                panic!("{}", msg);
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
            trap_unreachable: X64FunctionCode::emit_native_call_trampoline(
                &mut assembler,
                do_trap,
                0usize,
                TrapCode::Unreachable,
            ),
        };

        X64ModuleCodeGenerator {
            functions: vec![],
            signatures: None,
            function_signatures: None,
            assembler: Some(assembler),
            native_trampolines: Arc::new(nt),
        }
    }
}

impl ModuleCodeGenerator<X64FunctionCode, X64ExecutionContext> for X64ModuleCodeGenerator {
    fn next_function(&mut self) -> Result<&mut X64FunctionCode, CodegenError> {
        let (mut assembler, mut function_labels, br_table_data) = match self.functions.last_mut() {
            Some(x) => (
                x.assembler.take().unwrap(),
                x.function_labels.take().unwrap(),
                x.br_table_data.take().unwrap(),
            ),
            None => (self.assembler.take().unwrap(), HashMap::new(), vec![]),
        };
        let begin_label = *function_labels
            .entry(self.functions.len())
            .or_insert_with(|| assembler.new_dynamic_label());
        let begin_offset = assembler.offset();
        dynasm!(
            assembler
            ; => begin_label
            //; int 3
        );
        let code = X64FunctionCode {
            signatures: self.signatures.as_ref().unwrap().clone(),
            function_signatures: self.function_signatures.as_ref().unwrap().clone(),
            native_trampolines: self.native_trampolines.clone(),

            id: self.functions.len(),
            begin_label: begin_label,
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

    fn finalize(mut self) -> Result<X64ExecutionContext, CodegenError> {
        let (mut assembler, mut br_table_data) = match self.functions.last_mut() {
            Some(x) => (x.assembler.take().unwrap(), x.br_table_data.take().unwrap()),
            None => {
                return Err(CodegenError {
                    message: "no function",
                })
            }
        };
        let output = assembler.finalize().unwrap();

        for table in &mut br_table_data {
            for entry in table {
                *entry = output.ptr(AssemblyOffset(*entry)) as usize;
            }
        }
        Ok(X64ExecutionContext {
            code: output,
            functions: self.functions,
            br_table_data: br_table_data,
        })
    }

    fn feed_signatures(
        &mut self,
        signatures: Map<SigIndex, Arc<FuncSig>>,
    ) -> Result<(), CodegenError> {
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
}

impl X64FunctionCode {
    fn gen_rt_pop(assembler: &mut Assembler, info: &ValueInfo) -> Result<(), CodegenError> {
        match info.location {
            ValueLocation::Register(_) => {}
            ValueLocation::Stack => {
                let size = get_size_of_type(&info.ty)?;
                dynasm!(
                    assembler
                    ; add rsp, 8
                );
            }
        }
        Ok(())
    }

    /// Emits a unary operator.
    fn emit_unop_i32<F: FnOnce(&mut Assembler, &ValueStack, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
    ) -> Result<(), CodegenError> {
        let a = value_stack.pop()?;
        if a.ty != WpType::I32 {
            return Err(CodegenError {
                message: "unop(i32) type mismatch",
            });
        }
        value_stack.push(WpType::I32);

        match a.location {
            ValueLocation::Register(x) => {
                let reg = Register::from_scratch_reg(x);
                f(assembler, value_stack, reg);
            }
            ValueLocation::Stack => {
                dynasm!(
                    assembler
                    ; mov eax, [rsp]
                );
                f(assembler, value_stack, Register::RAX);
                dynasm!(
                    assembler
                    ; mov [rsp], eax
                );
            }
        }

        Ok(())
    }

    /// Emits a binary operator.
    ///
    /// Guarantees that the first Register parameter to callback `f` will never be `Register::RAX`.
    fn emit_binop_i32<F: FnOnce(&mut Assembler, &ValueStack, Register, Register)>(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        f: F,
    ) -> Result<(), CodegenError> {
        let (a, b) = value_stack.pop2()?;
        if a.ty != WpType::I32 || b.ty != WpType::I32 {
            return Err(CodegenError {
                message: "binop(i32) type mismatch",
            });
        }
        value_stack.push(WpType::I32);

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
                ; mov ecx, [rsp + 16]
                ; mov eax, [rsp + 8]
            );
            f(assembler, value_stack, Register::RCX, Register::RAX);
            dynasm!(
                assembler
                ; mov [rsp + 16], ecx
                ; pop rcx
                ; add rsp, 8
            );
        }

        Ok(())
    }

    fn emit_div_i32(
        assembler: &mut Assembler,
        value_stack: &ValueStack,
        left: Register,
        right: Register,
        signed: bool,
        out: Register,
    ) {
        let dx_used = Register::RDX.is_used(value_stack);
        if dx_used {
            dynasm!(
                assembler
                ; push rdx
            );
        }

        if right == Register::RAX {
            dynasm!(
                assembler
                ; push rax
                ; mov eax, Rd(left as u8)
                ; mov edx, 0
                ; mov Rd(left as u8), [rsp]
            );

            if signed {
                dynasm!(
                    assembler
                    ; idiv Rd(left as u8)
                );
            } else {
                dynasm!(
                    assembler
                    ; div Rd(left as u8)
                );
            }

            dynasm!(
                assembler
                ; mov Rd(left as u8), Rd(out as u8)
                ; pop rax
            );
        } else {
            dynasm!(
                assembler
                ; mov eax, Rd(left as u8)
                ; mov edx, 0
            );
            if signed {
                dynasm!(
                    assembler
                    ; idiv Rd(right as u8)
                );
            } else {
                dynasm!(
                    assembler
                    ; div Rd(right as u8)
                );
            }
            dynasm!(
                assembler
                ; mov Rd(left as u8), Rd(out as u8)
            );
        }

        if dx_used {
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

    fn emit_peek_into_ax(
        assembler: &mut Assembler,
        value_stack: &ValueStack,
    ) -> Result<(), CodegenError> {
        let val = match value_stack.values.last() {
            Some(x) => *x,
            None => {
                return Err(CodegenError {
                    message: "no value",
                })
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

        Ok(())
    }

    fn emit_pop_into_ax(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
    ) -> Result<WpType, CodegenError> {
        let val = value_stack.pop()?;
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
                    ; pop rax
                );
            }
        }

        Ok(val.ty)
    }

    fn emit_push_from_ax(
        assembler: &mut Assembler,
        value_stack: &mut ValueStack,
        ty: WpType,
    ) -> Result<(), CodegenError> {
        let loc = value_stack.push(ty);
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

        Ok(())
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
                })
            }
        };

        if value_stack.values.len() < frame.value_stack_depth_before + frame.returns.len() {
            return Err(CodegenError {
                message: "value stack underflow",
            });
        }

        if let Some(ty) = ret_ty {
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
                    message: "no frame",
                })
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
                })
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
                    message: "no frame",
                })
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
                        ; mov Rq(x as u8), rax
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
                })
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

    fn emit_native_call_trampoline<A: Copy + Sized, B: Copy + Sized>(
        assembler: &mut Assembler,
        target: unsafe extern "C" fn(
            ctx1: A,
            ctx2: B,
            stack_top: *mut u8,
            stack_base: *mut u8,
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
            ; mov rdi, QWORD (unsafe { ::std::mem::transmute_copy::<A, i64>(&ctx1) })
            ; mov rsi, QWORD (unsafe { ::std::mem::transmute_copy::<B, i64>(&ctx2) })
            ; mov rdx, rsp
            ; mov rcx, rbp
            ; push rbp
            ; mov rbp, rsp
            ; mov rax, QWORD (0xfffffffffffffff0u64 as i64)
            ; and rsp, rax
            ; mov rax, QWORD (target as i64)
            ; call rax
            ; mov rsp, rbp
            ; pop rbp
        );

        dynasm!(
            assembler
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
        for ty in params {
            let val = value_stack.pop()?;
            if val.ty != *ty {
                return Err(CodegenError {
                    message: "value type mismatch",
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
                })
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
        let assembler = self.assembler.as_mut().unwrap();

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
    fn feed_opcode(&mut self, op: Operator) -> Result<(), CodegenError> {
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
            Operator::I32Const { value } => {
                let location = self.value_stack.push(WpType::I32);
                match location {
                    ValueLocation::Register(x) => {
                        let reg = Register::from_scratch_reg(x);
                        dynasm!(
                            assembler
                            ; mov Rq(reg as u8), value
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
                        dynasm!(
                            assembler
                            ; or Rd(left as u8), Rd(right as u8)
                        );
                    },
                )?;
            }
            Operator::I32Eq => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
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
            Operator::I32Eqz => {
                Self::emit_unop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, reg| {
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
            // Comparison operators.
            // https://en.wikibooks.org/wiki/X86_Assembly/Control_Flow
            // TODO: Is reading flag register directly faster?
            Operator::I32LtS => {
                Self::emit_binop_i32(
                    assembler,
                    &mut self.value_stack,
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
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
                    |assembler, value_stack, left, right| {
                        Self::emit_cmp_i32(assembler, left, right, |assembler| {
                            dynasm!(
                                assembler
                                ; jae >label_true
                            );
                        });
                    },
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
                Self::emit_call_raw(
                    assembler,
                    &mut self.value_stack,
                    self.native_trampolines.trap_unreachable,
                    &[],
                    &[],
                )?;
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
                let label = *self
                    .function_labels
                    .as_mut()
                    .unwrap()
                    .entry(function_index)
                    .or_insert_with(|| assembler.new_dynamic_label());
                let sig_index = match self.function_signatures.get(FuncIndex::new(function_index)) {
                    Some(x) => *x,
                    None => {
                        return Err(CodegenError {
                            message: "signature not found",
                        })
                    }
                };
                let sig = match self.signatures.get(sig_index) {
                    Some(x) => x,
                    None => {
                        return Err(CodegenError {
                            message: "signature does not exist",
                        })
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
            Operator::End => {
                if self.control_stack.as_ref().unwrap().frames.len() == 1 {
                    let frame = self.control_stack.as_mut().unwrap().frames.pop().unwrap();

                    if !was_unreachable {
                        Self::emit_leave_frame(assembler, &frame, &mut self.value_stack, false)?;
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
            _ => unimplemented!(),
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

fn value_to_i64(v: &Value) -> i64 {
    match *v {
        Value::F32(x) => x.to_bits() as u64 as i64,
        Value::F64(x) => x.to_bits() as u64 as i64,
        Value::I32(x) => x as u64 as i64,
        Value::I64(x) => x as u64 as i64,
    }
}

fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
    }
}

unsafe extern "C" fn do_trap(
    ctx1: usize,
    ctx2: TrapCode,
    stack_top: *mut u8,
    stack_base: *mut u8,
) -> u64 {
    panic!("TRAP CODE: {:?}", ctx2);
}
