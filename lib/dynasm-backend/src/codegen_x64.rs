use super::codegen::*;
use super::stack::{ControlFrame, ControlStack, ValueInfo, ValueLocation, ValueStack};
use dynasmrt::{
    x64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, ExecutableBuffer,
};
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

#[derive(Default)]
pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
}

pub struct X64FunctionCode {
    id: usize,
    begin_label: DynamicLabel,
    begin_offset: AssemblyOffset,
    assembler: Option<Assembler>,
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

        match self.functions[index].num_params {
            0 => unsafe {
                let ptr: extern "C" fn() -> i64 = ::std::mem::transmute(ptr);
                Ok(vec![Value::I32(ptr() as i32)])
            },
            1 => unsafe {
                let ptr: extern "C" fn(i64) -> i64 = ::std::mem::transmute(ptr);
                Ok(vec![Value::I32(ptr(value_to_i64(&_params[0])) as i32)])
            },
            2 => unsafe {
                let ptr: extern "C" fn(i64, i64) -> i64 = ::std::mem::transmute(ptr);
                Ok(vec![Value::I32(
                    ptr(value_to_i64(&_params[0]), value_to_i64(&_params[1])) as i32,
                )])
            },
            _ => unimplemented!(),
        }
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
        X64ModuleCodeGenerator::default()
    }
}

impl ModuleCodeGenerator<X64FunctionCode, X64ExecutionContext> for X64ModuleCodeGenerator {
    fn next_function(&mut self) -> Result<&mut X64FunctionCode, CodegenError> {
        let mut assembler = match self.functions.last_mut() {
            Some(x) => x.assembler.take().unwrap(),
            None => match Assembler::new() {
                Ok(x) => x,
                Err(_) => {
                    return Err(CodegenError {
                        message: "cannot initialize assembler",
                    })
                }
            },
        };
        let begin_label = assembler.new_dynamic_label();
        let begin_offset = assembler.offset();
        dynasm!(
            assembler
            ; => begin_label
            ; push rbp
            ; mov rbp, rsp
            //; int 3
        );
        let code = X64FunctionCode {
            id: self.functions.len(),
            begin_label: begin_label,
            begin_offset: begin_offset,
            assembler: Some(assembler),
            returns: vec![],
            locals: vec![],
            num_params: 0,
            current_stack_offset: 0,
            value_stack: ValueStack::new(4),
            control_stack: None,
            unreachable_depth: 0,
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(mut self) -> Result<X64ExecutionContext, CodegenError> {
        let mut assembler = match self.functions.last_mut() {
            Some(x) => x.assembler.take().unwrap(),
            None => {
                return Err(CodegenError {
                    message: "no function",
                })
            }
        };
        let output = assembler.finalize().unwrap();
        Ok(X64ExecutionContext {
            code: output,
            functions: self.functions,
        })
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
                    ; add rsp, size as i32
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
                ; mov eax, [rsp]
                ; add rsp, 4
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
                ; mov ecx, [rsp + 12]
                ; mov eax, [rsp + 8]
            );
            f(assembler, value_stack, Register::RCX, Register::RAX);
            dynasm!(
                assembler
                ; mov [rsp + 12], ecx
                ; pop rcx
                ; add rsp, 4
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
                if is_dword(get_size_of_type(&val.ty)?) {
                    dynasm!(
                        assembler
                        ; mov eax, [rsp]
                    );
                } else {
                    dynasm!(
                        assembler
                        ; mov rax, [rsp]
                    );
                }
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
                if is_dword(get_size_of_type(&val.ty)?) {
                    dynasm!(
                        assembler
                        ; mov eax, [rsp]
                        ; add rsp, 4
                    );
                } else {
                    dynasm!(
                        assembler
                        ; mov rax, [rsp]
                        ; add rsp, 8
                    );
                }
            }
        }

        Ok(val.ty)
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
            dynasm!(
                assembler
                ; => frame.label
            );
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
                    if is_dword(get_size_of_type(&frame.returns[0])?) {
                        dynasm!(
                            assembler
                            ; sub rsp, 4
                            ; mov [rsp], eax
                        );
                    } else {
                        dynasm!(
                            assembler
                            ; sub rsp, 8
                            ; mov [rsp], rax
                        );
                    }
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
                sp_diff += get_size_of_type(&vi.ty)?;
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
}

impl FunctionCodeGenerator for X64FunctionCode {
    fn feed_return(&mut self, ty: WpType) -> Result<(), CodegenError> {
        self.returns.push(ty);
        Ok(())
    }

    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError> {
        let assembler = self.assembler.as_mut().unwrap();
        let size = get_size_of_type(&ty)?;

        self.current_stack_offset += size;
        self.locals.push(Local {
            ty: ty,
            stack_offset: self.current_stack_offset,
        });

        let param_reg = match self.num_params {
            0 => Register::RDI,
            1 => Register::RSI,
            2 => Register::RDX,
            3 => Register::RCX,
            4 => Register::R8,
            5 => Register::R9,
            _ => {
                return Err(CodegenError {
                    message: "more than 6 function parameters is not yet supported",
                })
            }
        };
        self.num_params += 1;

        if is_dword(size) {
            dynasm!(
                assembler
                ; sub rsp, 4
                ; mov [rsp], Rd(param_reg as u8)
            );
        } else {
            dynasm!(
                assembler
                ; sub rsp, 8
                ; mov [rsp], Rq(param_reg as u8)
            );
        }

        Ok(())
    }

    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError> {
        let assembler = self.assembler.as_mut().unwrap();
        let size = get_size_of_type(&ty)?;
        for _ in 0..n {
            // FIXME: check range of n
            self.current_stack_offset += size;
            self.locals.push(Local {
                ty: ty,
                stack_offset: self.current_stack_offset,
            });
            match size {
                4 => dynasm!(
                    assembler
                    ; sub rsp, 4
                    ; mov DWORD [rsp], 0
                ),
                8 => dynasm!(
                    assembler
                    ; sub rsp, 8
                    ; mov QWORD [rsp], 0
                ),
                _ => unreachable!(),
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
                                ; sub rsp, 4
                                ; mov [rsp], eax
                            );
                        } else {
                            dynasm!(
                                assembler
                                ; mov rax, [rbp - (local.stack_offset as i32)]
                                ; sub rsp, 8
                                ; mov [rsp], rax
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
                            ; sub rsp, 4
                            ; mov DWORD [rsp], value
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
