use super::codegen::*;
use super::stack::{ValueInfo, ValueLocation, ValueStack};
use dynasmrt::{x64::Assembler, DynamicLabel, DynasmApi, DynasmLabelApi};
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
}

#[derive(Default)]
pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
}

pub struct X64FunctionCode {
    id: usize,
    begin_label: DynamicLabel,
    cleanup_label: DynamicLabel,
    assembler: Option<Assembler>,
    returns: Vec<WpType>,
    locals: Vec<Local>,
    num_params: usize,
    current_stack_offset: usize,
    value_stack: ValueStack,
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

impl ModuleCodeGenerator<X64FunctionCode> for X64ModuleCodeGenerator {
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
        dynasm!(
            assembler
            ; => begin_label
            ; push rbp
            ; mov rbp, rsp
        );
        let code = X64FunctionCode {
            id: self.functions.len(),
            begin_label: begin_label,
            cleanup_label: assembler.new_dynamic_label(),
            assembler: Some(assembler),
            returns: vec![],
            locals: vec![],
            num_params: 0,
            current_stack_offset: 0,
            value_stack: ValueStack::new(8),
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        let mut assembler = match self.functions.last_mut() {
            Some(x) => x.assembler.take().unwrap(),
            None => return Ok(()),
        };
        let output = assembler.finalize().unwrap();
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
                    ; add rsp, size as i32
                );
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
                let (a, b) = self.value_stack.pop2()?;
                if a.ty != WpType::I32 || b.ty != WpType::I32 {
                    return Err(CodegenError {
                        message: "I32Add type mismatch",
                    });
                }
                Self::gen_rt_pop(assembler, &b)?;
                Self::gen_rt_pop(assembler, &a)?;

                self.value_stack.push(WpType::I32);

                if a.location.is_register() && b.location.is_register() {
                    let (a_reg, b_reg) = (
                        Register::from_scratch_reg(a.location.get_register()?),
                        Register::from_scratch_reg(b.location.get_register()?),
                    );
                    // output is in a_reg.
                    dynasm!(
                        assembler
                        ; add Rd(a_reg as u8), Rd(b_reg as u8)
                    );
                } else {
                    unimplemented!();
                }
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
