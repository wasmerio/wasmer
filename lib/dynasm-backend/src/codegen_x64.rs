use super::codegen::*;
use super::stack::{ValueInfo, ValueLocation, ValueStack};
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
    cleanup_label: DynamicLabel,
    assembler: Option<Assembler>,
    returns: Vec<WpType>,
    locals: Vec<Local>,
    num_params: usize,
    current_stack_offset: usize,
    value_stack: ValueStack,
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
            cleanup_label: assembler.new_dynamic_label(),
            assembler: Some(assembler),
            returns: vec![],
            locals: vec![],
            num_params: 0,
            current_stack_offset: 0,
            value_stack: ValueStack::new(4),
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
                message: "I32Add type mismatch",
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
        Ok(())
    }
    fn feed_opcode(&mut self, op: Operator) -> Result<(), CodegenError> {
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
            Operator::Drop => {
                let info = self.value_stack.pop()?;
                Self::gen_rt_pop(assembler, &info)?;
            }
            Operator::Return => match self.returns.len() {
                0 => {}
                1 => {
                    let val = self.value_stack.pop()?;
                    let ty = self.returns[0];
                    let reg = val.location.get_register()?;
                    if is_dword(get_size_of_type(&ty)?) {
                        dynasm!(
                            assembler
                            ; mov eax, Rd(Register::from_scratch_reg(reg) as u8)
                            ; jmp =>self.cleanup_label
                        );
                    } else {
                        dynasm!(
                            assembler
                            ; mov rax, Rq(Register::from_scratch_reg(reg) as u8)
                            ; jmp =>self.cleanup_label
                        );
                    }
                }
                _ => {
                    return Err(CodegenError {
                        message: "multiple return values is not yet supported",
                    })
                }
            },
            Operator::End => {
                // todo
            }
            _ => unimplemented!(),
        }
        Ok(())
    }
    fn finalize(&mut self) -> Result<(), CodegenError> {
        let assembler = self.assembler.as_mut().unwrap();
        dynasm!(
            assembler
            ; ud2
            ; => self.cleanup_label
            ; mov rsp, rbp
            ; pop rbp
            ; ret
        );
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
