use crate::instructions::{
    stack::{Stack, Stackable},
    wasm, Instruction,
};
use std::convert::TryFrom;

struct Runtime<'invocation, 'instance, Instance>
where
    Instance: wasm::Instance,
{
    invocation_inputs: &'invocation Vec<u64>,
    stack: Stack<u64>,
    wasm_instance: &'instance Instance,
}

pub(crate) struct Interpreter<Instance>
where
    Instance: wasm::Instance,
{
    executable_instructions: Vec<Box<dyn Fn(&mut Runtime<Instance>) -> Result<(), String>>>,
}

impl<Instance> Interpreter<Instance>
where
    Instance: wasm::Instance,
{
    fn iter(
        &self,
    ) -> impl Iterator<Item = &Box<dyn Fn(&mut Runtime<Instance>) -> Result<(), String>>> + '_ {
        self.executable_instructions.iter()
    }

    pub(crate) fn run(
        &self,
        invocation_inputs: &Vec<u64>,
        wasm_instance: &Instance,
    ) -> Result<Stack<u64>, String> {
        let mut runtime = Runtime {
            invocation_inputs,
            stack: Stack::new(),
            wasm_instance,
        };

        for executable_instruction in self.iter() {
            match executable_instruction(&mut runtime) {
                Ok(_) => continue,
                Err(message) => return Err(message),
            }
        }

        Ok(runtime.stack)
    }
}

impl<'binary_input, Instance> TryFrom<&Vec<Instruction<'binary_input>>> for Interpreter<Instance>
where
    Instance: wasm::Instance,
{
    type Error = String;

    fn try_from(instructions: &Vec<Instruction>) -> Result<Self, Self::Error> {
        let executable_instructions = instructions
            .iter()
            .map(
                |instruction| -> Box<dyn Fn(&mut Runtime<Instance>) -> Result<(), String>> {
                    match instruction {
                        Instruction::ArgumentGet(index) => {
                            let index = index.to_owned();
                            let instruction_name: String = instruction.into();

                            Box::new(move |runtime: &mut Runtime<Instance>| -> Result<(), _> {
                                let invocation_inputs = runtime.invocation_inputs;

                                if index >= (invocation_inputs.len() as u64) {
                                    return Err(format!(
                                        "`{}` cannot access argument #{} because it does't exist.",
                                        instruction_name, index
                                    ));
                                }

                                runtime.stack.push(invocation_inputs[index as usize]);

                                Ok(())
                            })
                        }
                        Instruction::CallExport(export_name) => {
                            let export_name = (*export_name).to_owned();

                            Box::new(move |_runtime: &mut Runtime<Instance>| -> Result<(), _> {
                                println!("call export {}", export_name);

                                Ok(())
                            })
                        }
                        Instruction::ReadUtf8 => {
                            Box::new(|_runtime: &mut Runtime<Instance>| -> Result<(), _> {
                                println!("read utf8");

                                Ok(())
                            })
                        }
                        Instruction::Call(index) => {
                            let index = index.to_owned();

                            Box::new(move |_runtime: &mut Runtime<Instance>| -> Result<(), _> {
                                println!("call {}", index);

                                Ok(())
                            })
                        }
                        _ => unimplemented!(),
                    }
                },
            )
            .collect();

        Ok(Interpreter {
            executable_instructions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Interpreter;
    use crate::instructions::{stack::Stackable, wasm, Instruction};
    use std::{collections::HashMap, convert::TryInto};

    struct Instance {
        exports: HashMap<String, ()>,
    }

    impl Instance {
        fn new() -> Self {
            Self {
                exports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert("foo".into(), ());

                    hashmap
                },
            }
        }
    }

    impl wasm::Instance for Instance {
        fn export_exists(&self, export_name: &str) -> bool {
            self.exports.contains_key(export_name)
        }
    }

    #[test]
    fn test_interpreter_from_instructions() {
        let instructions = vec![
            Instruction::ArgumentGet(0),
            Instruction::ArgumentGet(0),
            Instruction::CallExport("foo"),
            Instruction::ReadUtf8,
            Instruction::Call(7),
        ];
        let interpreter: Interpreter<()> = (&instructions).try_into().unwrap();

        assert_eq!(interpreter.executable_instructions.len(), 5);
    }

    #[test]
    fn test_interpreter_argument_get() {
        let interpreter: Interpreter<Instance> =
            (&vec![Instruction::ArgumentGet(0)]).try_into().unwrap();
        let invocation_inputs = vec![42];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_ok());

        let stack = run.unwrap();

        assert_eq!(stack.as_slice(), &[42]);
    }

    #[test]
    fn test_interpreter_argument_get_invalid_index() {
        let interpreter: Interpreter<Instance> =
            (&vec![Instruction::ArgumentGet(1)]).try_into().unwrap();
        let invocation_inputs = vec![42];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_err());

        let error = run.unwrap_err();

        assert_eq!(
            error,
            String::from("`arg.get 1` cannot access argument #1 because it does't exist.")
        );
    }

    #[test]
    fn test_interpreter_argument_get_argument_get() {
        let interpreter: Interpreter<Instance> =
            (&vec![Instruction::ArgumentGet(0), Instruction::ArgumentGet(1)])
                .try_into()
                .unwrap();
        let invocation_inputs = vec![7, 42];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_ok());

        let stack = run.unwrap();

        assert_eq!(stack.as_slice(), &[7, 42]);
    }

    /*
    #[test]
    fn test_interpreter_call_export() {
        let interpreter: Interpreter<Instance> =
            (&vec![Instruction::ArgumentGet(7), Instruction::ArgumentGet(42)])
                .try_into()
                .unwrap();
        let run = interpreter.run(&Instance::new());

        assert!(run.is_ok());

        let stack = run.unwrap();

        assert_eq!(stack.as_slice(), &[]);
    }
    */
}
