use super::codegen::*;
use super::stack::ValueStack;
use dynasmrt::{x64::Assembler, DynamicLabel, DynasmApi, DynasmLabelApi};
use wasmparser::{Operator, Type as WpType};

#[derive(Default)]
pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
}

pub struct X64FunctionCode {
    id: usize,
    begin_label: DynamicLabel,
    cleanup_label: DynamicLabel,
    assembler: Option<Assembler>,
    locals: Vec<Local>,
    num_params: usize,
    current_stack_offset: usize,
    callee_managed_stack_offset: usize,
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
        );
        let code = X64FunctionCode {
            id: self.functions.len(),
            begin_label: begin_label,
            cleanup_label: assembler.new_dynamic_label(),
            assembler: Some(assembler),
            locals: vec![],
            num_params: 0,
            current_stack_offset: 0,
            callee_managed_stack_offset: 0,
            value_stack: ValueStack::new(13),
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }
}

impl FunctionCodeGenerator for X64FunctionCode {
    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError> {
        let size = get_size_of_type(&ty)?;
        self.current_stack_offset += size;
        self.locals.push(Local {
            ty: ty,
            stack_offset: self.current_stack_offset,
        });
        self.num_params += 1;
        Ok(())
    }
    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError> {
        let size = get_size_of_type(&ty)?;
        for _ in 0..n {
            // FIXME: check range of n
            self.current_stack_offset += size;
            self.callee_managed_stack_offset += size;
            self.locals.push(Local {
                ty: ty,
                stack_offset: self.current_stack_offset,
            });
        }
        Ok(())
    }
    fn begin_body(&mut self) -> Result<(), CodegenError> {
        let assembler = self.assembler.as_mut().unwrap();
        dynasm!(
            assembler
            ; mov rax, rsp
            ; sub rsp, self.callee_managed_stack_offset as i32
            ; xor rcx, rcx
        );

        for local in &self.locals[self.num_params..] {
            let size = get_size_of_type(&local.ty)?;
            dynasm!(
                assembler
                ; sub rax, size as i32
            );
            if size == 4 {
                dynasm!(
                    assembler
                    ; mov [rax], ecx
                );
            } else if size == 8 {
                dynasm!(
                    assembler
                    ; mov [rax], rcx
                );
            } else {
                return Err(CodegenError {
                    message: "unsupported size for type",
                });
            }
        }
        dynasm!(
            assembler
            ; push rbp
            ; mov rbp, rsp
        );
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
                dynasm!(
                    assembler
                    ; mov rax, rbp
                    ; add rax, (self.current_stack_offset - local.stack_offset) as i32
                    // TODO: How should we dynamically specify a register?
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
            ; ud2
            ; => self.cleanup_label
            ; mov rsp, rbp
            ; pop rbp
            ; add rsp, self.current_stack_offset as i32
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
