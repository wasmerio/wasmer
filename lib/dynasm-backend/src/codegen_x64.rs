use super::codegen::*;
use dynasmrt::{x64::Assembler, DynasmApi};
use wasmparser::{Operator, Type as WpType};

#[derive(Default)]
pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
}

pub struct X64FunctionCode {
    assembler: Option<Assembler>,
    locals: Vec<Local>,
    current_stack_offset: usize,
}

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
        let code = X64FunctionCode {
            assembler: Some(match self.functions.last_mut() {
                Some(x) => x.assembler.take().unwrap(),
                None => match Assembler::new() {
                    Ok(x) => x,
                    Err(_) => {
                        return Err(CodegenError {
                            message: "cannot initialize assembler",
                        })
                    }
                },
            }),
            locals: vec![],
            current_stack_offset: 0,
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }
}

impl FunctionCodeGenerator for X64FunctionCode {
    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError> {
        let size = get_size_of_type(&ty)?;
        self.current_stack_offset -= size;
        self.locals.push(Local {
            ty: ty,
            stack_offset: self.current_stack_offset,
        });
        // TODO: load parameter values onto stack...
        Ok(())
    }
    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError> {
        let size = get_size_of_type(&ty)?;
        let assembler = self.assembler.as_mut().unwrap();

        dynasm!(
            assembler
            ; xor rax, rax
        );

        for _ in 0..n {
            // FIXME: check range of n
            self.current_stack_offset -= size;
            self.locals.push(Local {
                ty: ty,
                stack_offset: self.current_stack_offset,
            });
            dynasm!(
                assembler
                ; mov [rsp - (self.current_stack_offset as i32)], rax
            );
        }
        Ok(())
    }
    fn feed_opcode(&mut self, op: Operator) -> Result<(), CodegenError> {
        Ok(())
    }
    fn finalize(&mut self) -> Result<(), CodegenError> {
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
