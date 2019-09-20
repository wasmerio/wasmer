use crate::instructions::{
    stack::{Stack, Stackable},
    wasm::{self, Type, Value},
    Instruction,
};
use std::{convert::TryFrom, marker::PhantomData};

struct Runtime<'invocation, 'instance, Instance, Export>
where
    Export: wasm::Export + 'instance,
    Instance: wasm::Instance<Export> + 'instance,
{
    invocation_inputs: &'invocation [Value],
    stack: Stack<Value>,
    wasm_instance: &'instance Instance,
    wasm_exports: PhantomData<Export>,
}

type ExecutableInstruction<Instance, Export> =
    Box<dyn Fn(&mut Runtime<Instance, Export>) -> Result<(), String>>;

pub struct Interpreter<Instance, Export>
where
    Export: wasm::Export,
    Instance: wasm::Instance<Export>,
{
    executable_instructions: Vec<ExecutableInstruction<Instance, Export>>,
}

impl<Instance, Export> Interpreter<Instance, Export>
where
    Export: wasm::Export,
    Instance: wasm::Instance<Export>,
{
    fn iter(&self) -> impl Iterator<Item = &ExecutableInstruction<Instance, Export>> + '_ {
        self.executable_instructions.iter()
    }

    pub fn run(
        &self,
        invocation_inputs: &[Value],
        wasm_instance: &Instance,
    ) -> Result<Stack<Value>, String> {
        let mut runtime = Runtime {
            invocation_inputs,
            stack: Stack::new(),
            wasm_instance,
            wasm_exports: PhantomData,
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

impl<'binary_input, Instance, Export> TryFrom<&Vec<Instruction<'binary_input>>>
    for Interpreter<Instance, Export>
where
    Export: wasm::Export,
    Instance: wasm::Instance<Export>,
{
    type Error = String;

    fn try_from(instructions: &Vec<Instruction>) -> Result<Self, Self::Error> {
        let executable_instructions = instructions
            .iter()
            .map(
                |instruction| -> ExecutableInstruction<Instance, Export> {
                    match instruction {
                        Instruction::ArgumentGet(index) => {
                            let index = index.to_owned();
                            let instruction_name: String = instruction.into();

                            Box::new(move |runtime: &mut Runtime<Instance, Export>| -> Result<(), _> {
                                let invocation_inputs = runtime.invocation_inputs;

                                if index >= (invocation_inputs.len() as u64) {
                                    return Err(format!(
                                        "`{}` cannot access argument #{} because it doesn't exist.",
                                        instruction_name, index
                                    ));
                                }

                                runtime.stack.push(invocation_inputs[index as usize]);

                                Ok(())
                            })
                        }
                        Instruction::CallExport(export_name) => {
                            let export_name = (*export_name).to_owned();
                            let instruction_name: String = instruction.into();

                            Box::new(move |runtime: &mut Runtime<Instance, Export>| -> Result<(), _> {
                                let instance = runtime.wasm_instance;

                                match instance.export(&export_name) {
                                    Some(export) => {
                                        let inputs_cardinality = export.inputs_cardinality();

                                        match runtime.stack.pop(inputs_cardinality) {
                                            Some(inputs) =>  {
                                                let input_types = inputs
                                                    .iter()
                                                    .map(|input| input.into())
                                                    .collect::<Vec<Type>>();

                                                if input_types != export.inputs() {
                                                    return Err(format!(
                                                        "`{}` cannot call the exported function `{}` because the value types in the stack mismatch the function signature (expects {:?}).",
                                                        instruction_name,
                                                        export_name,
                                                        export.inputs(),
                                                    ))
                                                }

                                                match export.call(&inputs) {
                                                    Ok(outputs) => {
                                                        for output in outputs.iter() {
                                                            runtime.stack.push(*output);
                                                        }

                                                        Ok(())
                                                    }
                                                    Err(_) => Err(format!(
                                                        "`{}` failed when calling the exported function `{}`.",
                                                        instruction_name,
                                                        export_name
                                                    ))
                                                }
                                            }
                                            None => Err(format!(
                                                "`{}` cannot call the exported function `{}` because there is no enought data in the stack for the arguments (needs {}).",
                                                instruction_name,
                                                export_name,
                                                inputs_cardinality,
                                            ))
                                        }
                                    }
                                    None => Err(format!(
                                        "`{}` cannot call the exported function `{}` because it doesn't exist.",
                                        instruction_name,
                                        export_name,
                                    ))
                                }
                            })
                        }
                        Instruction::ReadUtf8 => {
                            Box::new(|_runtime: &mut Runtime<Instance, Export>| -> Result<(), _> {
                                println!("read utf8");

                                Ok(())
                            })
                        }
                        Instruction::Call(index) => {
                            let index = index.to_owned();

                            Box::new(move |_runtime: &mut Runtime<Instance, Export>| -> Result<(), _> {
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
    use crate::instructions::{
        stack::Stackable,
        wasm::{self, Type, Value},
        Instruction,
    };
    use std::{collections::HashMap, convert::TryInto};

    struct Export {
        inputs: Vec<Type>,
        outputs: Vec<Type>,
        function: fn(arguments: &[Value]) -> Result<Vec<Value>, ()>,
    }

    impl wasm::Export for Export {
        fn inputs_cardinality(&self) -> usize {
            self.inputs.len() as usize
        }

        fn outputs_cardinality(&self) -> usize {
            self.outputs.len()
        }

        fn inputs(&self) -> &[Type] {
            &self.inputs
        }

        fn outputs(&self) -> &[Type] {
            &self.outputs
        }

        fn call(&self, arguments: &[Value]) -> Result<Vec<Value>, ()> {
            (self.function)(arguments)
        }
    }

    struct Instance {
        exports: HashMap<String, Export>,
    }

    impl Instance {
        fn new() -> Self {
            Self {
                exports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        "sum".into(),
                        Export {
                            inputs: vec![Type::I32, Type::I32],
                            outputs: vec![Type::I32],
                            function: |arguments: &[Value]| {
                                let a: i32 = (&arguments[0]).try_into().unwrap();
                                let b: i32 = (&arguments[1]).try_into().unwrap();

                                Ok(vec![Value::I32(a + b)])
                            },
                        },
                    );

                    hashmap
                },
            }
        }
    }

    impl wasm::Instance<Export> for Instance {
        fn export(&self, export_name: &str) -> Option<&Export> {
            self.exports.get(export_name)
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
        let interpreter: Interpreter<(), ()> = (&instructions).try_into().unwrap();

        assert_eq!(interpreter.executable_instructions.len(), 5);
    }

    #[test]
    fn test_interpreter_argument_get() {
        let interpreter: Interpreter<Instance, Export> =
            (&vec![Instruction::ArgumentGet(0)]).try_into().unwrap();

        let invocation_inputs = vec![Value::I32(42)];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_ok());

        let stack = run.unwrap();

        assert_eq!(stack.as_slice(), &[Value::I32(42)]);
    }

    #[test]
    fn test_interpreter_argument_get_invalid_index() {
        let interpreter: Interpreter<Instance, Export> =
            (&vec![Instruction::ArgumentGet(1)]).try_into().unwrap();

        let invocation_inputs = vec![Value::I32(42)];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_err());

        let error = run.unwrap_err();

        assert_eq!(
            error,
            String::from("`arg.get 1` cannot access argument #1 because it doesn't exist.")
        );
    }

    #[test]
    fn test_interpreter_argument_get_argument_get() {
        let interpreter: Interpreter<Instance, Export> =
            (&vec![Instruction::ArgumentGet(0), Instruction::ArgumentGet(1)])
                .try_into()
                .unwrap();

        let invocation_inputs = vec![Value::I32(7), Value::I32(42)];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_ok());

        let stack = run.unwrap();

        assert_eq!(stack.as_slice(), &[Value::I32(7), Value::I32(42)]);
    }

    #[test]
    fn test_interpreter_call_export() {
        let interpreter: Interpreter<Instance, Export> = (&vec![
            Instruction::ArgumentGet(1),
            Instruction::ArgumentGet(0),
            Instruction::CallExport("sum"),
        ])
            .try_into()
            .unwrap();

        let invocation_inputs = vec![Value::I32(3), Value::I32(4)];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_ok());

        let stack = run.unwrap();

        assert_eq!(stack.as_slice(), &[Value::I32(7)]);
    }

    #[test]
    fn test_interpreter_call_export_invalid_export_name() {
        let interpreter: Interpreter<Instance, Export> =
            (&vec![Instruction::CallExport("bar")]).try_into().unwrap();

        let invocation_inputs = vec![];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_err());

        let error = run.unwrap_err();

        assert_eq!(
            error,
            String::from(r#"`call-export "bar"` cannot call the exported function `bar` because it doesn't exist."#)
        );
    }

    #[test]
    fn test_interpreter_call_export_too_small_stack() {
        let interpreter: Interpreter<Instance, Export> = (&vec![
            Instruction::ArgumentGet(0),
            Instruction::CallExport("sum"),
            //                       ^^^ `sum` expects 2 values in the stack, only one is present
        ])
            .try_into()
            .unwrap();

        let invocation_inputs = vec![Value::I32(3), Value::I32(4)];
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_err());

        let error = run.unwrap_err();

        assert_eq!(
            error,
            String::from(r#"`call-export "sum"` cannot call the exported function `sum` because there is no enought data in the stack for the arguments (needs 2)."#)
        );
    }

    #[test]
    fn test_interpreter_call_export_invalid_types_in_the_stack() {
        let interpreter: Interpreter<Instance, Export> = (&vec![
            Instruction::ArgumentGet(1),
            Instruction::ArgumentGet(0),
            Instruction::CallExport("sum"),
        ])
            .try_into()
            .unwrap();

        let invocation_inputs = vec![Value::I32(3), Value::I64(4)];
        //                                                 ^^^ mismatch with `sum` signature
        let instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_err());

        let error = run.unwrap_err();

        assert_eq!(
            error,
            String::from(r#"`call-export "sum"` cannot call the exported function `sum` because the value types in the stack mismatch the function signature (expects [I32, I32])."#)
        );
    }

    #[test]
    fn test_interpreter_call_export_failed_when_calling() {
        let interpreter: Interpreter<Instance, Export> = (&vec![
            Instruction::ArgumentGet(1),
            Instruction::ArgumentGet(0),
            Instruction::CallExport("sum"),
        ])
            .try_into()
            .unwrap();

        let invocation_inputs = vec![Value::I32(3), Value::I32(4)];
        let instance = Instance {
            exports: {
                let mut hashmap = HashMap::new();
                hashmap.insert(
                    "sum".into(),
                    Export {
                        inputs: vec![Type::I32, Type::I32],
                        outputs: vec![Type::I32],
                        function: |_| Err(()),
                        //            ^^^^^^ function fails
                    },
                );

                hashmap
            },
        };
        let run = interpreter.run(&invocation_inputs, &instance);

        assert!(run.is_err());

        let error = run.unwrap_err();

        assert_eq!(
            error,
            String::from(r#"`call-export "sum"` failed when calling the exported function `sum`."#)
        );
    }
}
