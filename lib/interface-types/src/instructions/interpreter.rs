use crate::instructions::{stack::Stack, Instruction};
use std::convert::TryFrom;

type ExecutableInstruction = Box<dyn Fn(&mut Stack)>;

pub(crate) struct Interpreter {
    stack: Stack,
    instructions: Vec<ExecutableInstruction>,
}

impl Interpreter {
    pub(crate) fn is_eos(&self) -> bool {
        self.stack.is_empty()
    }
}

impl<'binary_input> TryFrom<&Vec<Instruction<'binary_input>>> for Interpreter {
    type Error = &'static str;

    fn try_from(instructions: &Vec<Instruction>) -> Result<Self, Self::Error> {
        let executable_instructions = instructions
            .iter()
            .map(|instruction| -> ExecutableInstruction {
                match instruction {
                    Instruction::ArgumentGet(index) => {
                        let index = index.to_owned();

                        Box::new(move |stack: &mut Stack| {
                            println!("argument get {}", index);
                        })
                    }
                    Instruction::CallExport(export_name) => {
                        let export_name = (*export_name).to_owned();

                        Box::new(move |stack: &mut Stack| {
                            println!("call export {}", export_name);
                        })
                    }
                    Instruction::ReadUtf8 => Box::new(|stack: &mut Stack| {
                        println!("read utf8");
                    }),
                    Instruction::Call(index) => {
                        let index = index.to_owned();

                        Box::new(move |stack: &mut Stack| {
                            println!("call {}", index);
                        })
                    }
                    _ => unimplemented!(),
                }
            })
            .collect();

        Ok(Interpreter {
            stack: Stack::new(),
            instructions: executable_instructions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Interpreter;
    use crate::instructions::Instruction;
    use std::convert::TryInto;

    #[test]
    fn test_interpreter_from_instructions() {
        let instructions = vec![
            Instruction::ArgumentGet(0),
            Instruction::ArgumentGet(0),
            Instruction::CallExport("strlen"),
            Instruction::ReadUtf8,
            Instruction::Call(7),
        ];
        let interpreter: Result<Interpreter, _> = (&instructions).try_into();
        assert!(interpreter.is_ok());

        let interpreter = interpreter.unwrap();
        assert_eq!(interpreter.is_eos(), true);
    }
}
