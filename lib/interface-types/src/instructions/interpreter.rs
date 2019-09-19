use crate::instructions::{
    stack::{Stack, Stackable},
    Instruction,
};
use std::convert::TryFrom;

type ExecutableInstruction = Box<dyn Fn(&mut Stack<u64>) -> Result<(), &'static str>>;

pub(crate) struct Interpreter {
    executable_instructions: Vec<ExecutableInstruction>,
}

impl Interpreter {
    fn iter(&self) -> impl Iterator<Item = &ExecutableInstruction> + '_ {
        self.executable_instructions.iter()
    }

    pub(crate) fn run(&self) -> Result<Stack<u64>, &'static str> {
        let mut stack = Stack::new();

        for executable_instruction in self.iter() {
            match executable_instruction(&mut stack) {
                Ok(_) => continue,
                Err(message) => return Err(message),
            }
        }

        Ok(stack)
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

                        Box::new(move |stack: &mut Stack<u64>| -> Result<(), _> {
                            println!("argument get {}", index);
                            stack.push(index);

                            Ok(())
                        })
                    }
                    Instruction::CallExport(export_name) => {
                        let export_name = (*export_name).to_owned();

                        Box::new(move |_stack: &mut Stack<u64>| -> Result<(), _> {
                            println!("call export {}", export_name);

                            Ok(())
                        })
                    }
                    Instruction::ReadUtf8 => Box::new(|_stack: &mut Stack<u64>| -> Result<(), _> {
                        println!("read utf8");

                        Ok(())
                    }),
                    Instruction::Call(index) => {
                        let index = index.to_owned();

                        Box::new(move |_stack: &mut Stack<u64>| -> Result<(), _> {
                            println!("call {}", index);

                            Ok(())
                        })
                    }
                    _ => unimplemented!(),
                }
            })
            .collect();

        Ok(Interpreter {
            executable_instructions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Interpreter;
    use crate::instructions::{stack::Stackable, Instruction};
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
        let interpreter: Interpreter = (&instructions).try_into().unwrap();

        assert_eq!(interpreter.executable_instructions.len(), 5);
    }

    #[test]
    fn test_interpreter_argument_get() {
        let interpreter: Interpreter = (&vec![Instruction::ArgumentGet(42)]).try_into().unwrap();
        let run = interpreter.run();

        assert!(run.is_ok());

        let stack = run.unwrap();

        assert_eq!(stack.as_slice(), &[42]);
    }
}
