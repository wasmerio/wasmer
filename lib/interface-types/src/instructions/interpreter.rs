use crate::instructions::{
    stack::{Stack, Stackable},
    wasm::{self, FunctionIndex, InterfaceType, InterfaceValue, TypedIndex},
    Instruction,
};
use std::{cell::Cell, convert::TryFrom, marker::PhantomData};

struct Runtime<'invocation, 'instance, Instance, Export, LocalImport, Memory>
where
    Export: wasm::Export + 'instance,
    LocalImport: wasm::LocalImport + 'instance,
    Memory: wasm::Memory + 'instance,
    Instance: wasm::Instance<Export, LocalImport, Memory> + 'instance,
{
    invocation_inputs: &'invocation [InterfaceValue],
    stack: Stack<InterfaceValue>,
    wasm_instance: &'instance Instance,
    wasm_exports: PhantomData<Export>,
    wasm_locals_or_imports: PhantomData<LocalImport>,
    wasm_memories: PhantomData<Memory>,
}

type ExecutableInstruction<Instance, Export, LocalImport, Memory> =
    Box<dyn Fn(&mut Runtime<Instance, Export, LocalImport, Memory>) -> Result<(), String>>;

pub struct Interpreter<Instance, Export, LocalImport, Memory>
where
    Export: wasm::Export,
    LocalImport: wasm::LocalImport,
    Memory: wasm::Memory,
    Instance: wasm::Instance<Export, LocalImport, Memory>,
{
    executable_instructions: Vec<ExecutableInstruction<Instance, Export, LocalImport, Memory>>,
}

impl<Instance, Export, LocalImport, Memory> Interpreter<Instance, Export, LocalImport, Memory>
where
    Export: wasm::Export,
    LocalImport: wasm::LocalImport,
    Memory: wasm::Memory,
    Instance: wasm::Instance<Export, LocalImport, Memory>,
{
    fn iter(
        &self,
    ) -> impl Iterator<Item = &ExecutableInstruction<Instance, Export, LocalImport, Memory>> + '_
    {
        self.executable_instructions.iter()
    }

    pub fn run(
        &self,
        invocation_inputs: &[InterfaceValue],
        wasm_instance: &Instance,
    ) -> Result<Stack<InterfaceValue>, String> {
        let mut runtime = Runtime {
            invocation_inputs,
            stack: Stack::new(),
            wasm_instance,
            wasm_exports: PhantomData,
            wasm_locals_or_imports: PhantomData,
            wasm_memories: PhantomData,
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

impl<'binary_input, Instance, Export, LocalImport, Memory> TryFrom<&Vec<Instruction<'binary_input>>>
    for Interpreter<Instance, Export, LocalImport, Memory>
where
    Export: wasm::Export,
    LocalImport: wasm::LocalImport,
    Memory: wasm::Memory,
    Instance: wasm::Instance<Export, LocalImport, Memory>,
{
    type Error = String;

    fn try_from(instructions: &Vec<Instruction>) -> Result<Self, Self::Error> {
        let executable_instructions = instructions
            .iter()
            .map(
                |instruction| -> ExecutableInstruction<Instance, Export, LocalImport, Memory> {
                    match instruction {
                        Instruction::ArgumentGet { index } => {
                            let index = index.to_owned();
                            let instruction_name: String = instruction.into();

                            Box::new(move |runtime: &mut Runtime<Instance, Export, LocalImport, Memory>| -> Result<(), _> {
                                let invocation_inputs = runtime.invocation_inputs;

                                if index >= (invocation_inputs.len() as u64) {
                                    return Err(format!(
                                        "`{}` cannot access argument #{} because it doesn't exist.",
                                        instruction_name, index
                                    ));
                                }

                                runtime.stack.push(invocation_inputs[index as usize].clone());

                                Ok(())
                            })
                        }
                        Instruction::CallExport { export_name } => {
                            let export_name = (*export_name).to_owned();
                            let instruction_name: String = instruction.into();

                            Box::new(move |runtime: &mut Runtime<Instance, Export, LocalImport, Memory>| -> Result<(), _> {
                                let instance = runtime.wasm_instance;

                                match instance.export(&export_name) {
                                    Some(export) => {
                                        let inputs_cardinality = export.inputs_cardinality();

                                        match runtime.stack.pop(inputs_cardinality) {
                                            Some(inputs) =>  {
                                                let input_types = inputs
                                                    .iter()
                                                    .map(|input| input.into())
                                                    .collect::<Vec<InterfaceType>>();

                                                if input_types != export.inputs() {
                                                    return Err(format!(
                                                        "`{}` cannot call the exported function `{}` because the value types on the stack mismatch the function signature (expects {:?}).",
                                                        instruction_name,
                                                        export_name,
                                                        export.inputs(),
                                                    ))
                                                }

                                                match export.call(&inputs) {
                                                    Ok(outputs) => {
                                                        for output in outputs.iter() {
                                                            runtime.stack.push(output.clone());
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
                                                "`{}` cannot call the exported function `{}` because there is no enough data on the stack for the arguments (needs {}).",
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
                            let instruction_name: String = instruction.into();

                            Box::new(move |runtime: &mut Runtime<Instance, Export, LocalImport, Memory>| -> Result<(), _> {
                                match runtime.stack.pop(2) {
                                    Some(inputs) => match runtime.wasm_instance.memory(0) {
                                        Some(memory) => {
                                            let length = i32::try_from(&inputs[0])? as usize;
                                            let pointer = i32::try_from(&inputs[1])? as usize;
                                            let memory_view = memory.view::<u8>();

                                            if memory_view.len() < pointer + length {
                                                return Err(format!(
                                                    "`{}` failed because it has to read out of the memory bounds (index {} > memory length {}).",
                                                    instruction_name,
                                                    pointer + length,
                                                    memory_view.len()
                                                ));
                                            }

                                            let data: Vec<u8> = (&memory_view[pointer..pointer + length])
                                                .iter()
                                                .map(Cell::get)
                                                .collect();

                                            match String::from_utf8(data) {
                                                Ok(string) => {
                                                    runtime.stack.push(InterfaceValue::String(string));

                                                    Ok(())
                                                }
                                                Err(utf8_error) => Err(format!(
                                                    "`{}` failed because the read string isn't UTF-8 valid ({}).",
                                                    instruction_name,
                                                    utf8_error,
                                                ))
                                            }
                                        }
                                        None => Err(format!(
                                            "`{}` failed because there is no memory to read.",
                                            instruction_name
                                        ))
                                    }
                                    None => Err(format!(
                                        "`{}` failed because there is no enough data on the stack (needs 2).",
                                        instruction_name,
                                    ))
                                }
                            })
                        }
                        Instruction::Call { function_index: index } => {
                            let index = index.to_owned();
                            let instruction_name: String = instruction.into();

                            Box::new(move |runtime: &mut Runtime<Instance, Export, LocalImport, Memory>| -> Result<(), _> {
                                let instance = runtime.wasm_instance;
                                let function_index = FunctionIndex::new(index);

                                match instance.local_or_import(function_index) {
                                    Some(local_or_import) => {
                                        let inputs_cardinality = local_or_import.inputs_cardinality();

                                        match runtime.stack.pop(inputs_cardinality) {
                                            Some(inputs) =>  {
                                                let input_types = inputs
                                                    .iter()
                                                    .map(|input| input.into())
                                                    .collect::<Vec<InterfaceType>>();

                                                if input_types != local_or_import.inputs() {
                                                    return Err(format!(
                                                        "`{}` cannot call the local or imported function `{}` because the value types on the stack mismatch the function signature (expects {:?}).",
                                                        instruction_name,
                                                        index,
                                                        local_or_import.inputs(),
                                                    ))
                                                }

                                                match local_or_import.call(&inputs) {
                                                    Ok(outputs) => {
                                                        for output in outputs.iter() {
                                                            runtime.stack.push(output.clone());
                                                        }

                                                        Ok(())
                                                    }
                                                    Err(_) => Err(format!(
                                                        "`{}` failed when calling the local or imported function `{}`.",
                                                        instruction_name,
                                                        index
                                                    ))
                                                }
                                            }
                                            None => Err(format!(
                                                "`{}` cannot call the local or imported function `{}` because there is no enough data on the stack for the arguments (needs {}).",
                                                instruction_name,
                                                index,
                                                inputs_cardinality,
                                            ))
                                        }
                                    }
                                    None => Err(format!(
                                        "`{}` cannot call the local or imported function `{}` because it doesn't exist.",
                                        instruction_name,
                                        index,
                                    ))
                                }
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
        wasm::{self, InterfaceType, InterfaceValue},
        Instruction,
    };
    use std::{cell::Cell, collections::HashMap, convert::TryInto};

    struct Export {
        inputs: Vec<InterfaceType>,
        outputs: Vec<InterfaceType>,
        function: fn(arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>,
    }

    impl wasm::Export for Export {
        fn inputs_cardinality(&self) -> usize {
            self.inputs.len() as usize
        }

        fn outputs_cardinality(&self) -> usize {
            self.outputs.len()
        }

        fn inputs(&self) -> &[InterfaceType] {
            &self.inputs
        }

        fn outputs(&self) -> &[InterfaceType] {
            &self.outputs
        }

        fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
            (self.function)(arguments)
        }
    }

    struct LocalImport {
        inputs: Vec<InterfaceType>,
        outputs: Vec<InterfaceType>,
        function: fn(arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>,
    }

    impl wasm::LocalImport for LocalImport {
        fn inputs_cardinality(&self) -> usize {
            self.inputs.len() as usize
        }

        fn outputs_cardinality(&self) -> usize {
            self.outputs.len()
        }

        fn inputs(&self) -> &[InterfaceType] {
            &self.inputs
        }

        fn outputs(&self) -> &[InterfaceType] {
            &self.outputs
        }

        fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
            (self.function)(arguments)
        }
    }

    #[derive(Default)]
    struct Memory {
        data: Vec<Cell<u8>>,
    }

    impl Memory {
        fn new(data: Vec<Cell<u8>>) -> Self {
            Self { data }
        }
    }

    impl wasm::Memory for Memory {
        fn view<V: wasm::ValueType>(&self) -> &[Cell<V>] {
            let slice = self.data.as_slice();

            unsafe { ::std::slice::from_raw_parts(slice.as_ptr() as *const Cell<V>, slice.len()) }
        }
    }

    #[derive(Default)]
    struct Instance {
        exports: HashMap<String, Export>,
        locals_or_imports: HashMap<usize, LocalImport>,
        memory: Memory,
    }

    impl Instance {
        fn new() -> Self {
            Self {
                exports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        "sum".into(),
                        Export {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |arguments: &[InterfaceValue]| {
                                let a: i32 = (&arguments[0]).try_into().unwrap();
                                let b: i32 = (&arguments[1]).try_into().unwrap();

                                Ok(vec![InterfaceValue::I32(a + b)])
                            },
                        },
                    );

                    hashmap
                },
                locals_or_imports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        42,
                        LocalImport {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |arguments: &[InterfaceValue]| {
                                let a: i32 = (&arguments[0]).try_into().unwrap();
                                let b: i32 = (&arguments[1]).try_into().unwrap();

                                Ok(vec![InterfaceValue::I32(a * b)])
                            },
                        },
                    );

                    hashmap
                },
                memory: Memory::new(vec![]),
            }
        }
    }

    impl wasm::Instance<Export, LocalImport, Memory> for Instance {
        fn export(&self, export_name: &str) -> Option<&Export> {
            self.exports.get(export_name)
        }

        fn local_or_import<I: wasm::TypedIndex + wasm::LocalImportIndex>(
            &self,
            index: I,
        ) -> Option<&LocalImport> {
            self.locals_or_imports.get(&index.index())
        }

        fn memory(&self, _index: usize) -> Option<&Memory> {
            Some(&self.memory)
        }
    }

    #[test]
    fn test_interpreter_from_instructions() {
        let instructions = vec![
            Instruction::ArgumentGet { index: 0 },
            Instruction::ArgumentGet { index: 0 },
            Instruction::CallExport { export_name: "foo" },
            Instruction::ReadUtf8,
            Instruction::Call { function_index: 7 },
        ];
        let interpreter: Interpreter<(), (), (), ()> = (&instructions).try_into().unwrap();

        assert_eq!(interpreter.executable_instructions.len(), 5);
    }

    macro_rules! test {
        (
            $test_name:ident =
                instructions: [ $($instructions:expr),* $(,)* ],
                invocation_inputs: [ $($invocation_inputs:expr),* $(,)* ],
                instance: $instance:expr,
                stack: [ $($stack:expr),* $(,)* ]
                $(,)*
        ) => {
            #[test]
            #[allow(non_snake_case)]
            fn $test_name() {
                let interpreter: Interpreter<Instance, Export, LocalImport, Memory> =
                    (&vec![$($instructions),*]).try_into().unwrap();

                let invocation_inputs = vec![$($invocation_inputs),*];
                let instance = $instance;
                let run = interpreter.run(&invocation_inputs, &instance);

                assert!(run.is_ok());

                let stack = run.unwrap();

                assert_eq!(stack.as_slice(), &[$($stack),*]);
            }
        };

        (
            $test_name:ident =
                instructions: [ $($instructions:expr),* $(,)* ],
                invocation_inputs: [ $($invocation_inputs:expr),* $(,)* ],
                instance: $instance:expr,
                error: $error:expr
                $(,)*
        ) => {
            #[test]
            #[allow(non_snake_case)]
            fn $test_name() {
                let interpreter: Interpreter<Instance, Export, LocalImport, Memory> =
                    (&vec![$($instructions),*]).try_into().unwrap();

                let invocation_inputs = vec![$($invocation_inputs),*];
                let instance = $instance;
                let run = interpreter.run(&invocation_inputs, &instance);

                assert!(run.is_err());

                let error = run.unwrap_err();

                assert_eq!(error, String::from($error));
            }
        };
    }

    test!(
        test_interpreter_argument_get =
            instructions: [Instruction::ArgumentGet { index: 0 }],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test!(
        test_interpreter_argument_get__twice =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(7),
                InterfaceValue::I32(42),
            ],
            instance: Instance::new(),
            stack: [
                InterfaceValue::I32(7),
                InterfaceValue::I32(42),
            ],
    );

    test!(
        test_interpreter_argument_get__invalid_index =
            instructions: [Instruction::ArgumentGet { index: 1 }],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            error: "`arg.get 1` cannot access argument #1 because it doesn't exist."
    );

    test!(
        test_interpreter_call_export =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallExport { export_name: "sum" },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(7)],
    );

    test!(
        test_interpreter_call_export__invalid_export_name =
            instructions: [Instruction::CallExport { export_name: "bar" }],
            invocation_inputs: [],
            instance: Instance::new(),
            error: r#"`call-export "bar"` cannot call the exported function `bar` because it doesn't exist."#,
    );

    test!(
        test_interpreter_call_export__stack_is_too_small =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallExport { export_name: "sum" },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance::new(),
            error: r#"`call-export "sum"` cannot call the exported function `sum` because there is no enough data on the stack for the arguments (needs 2)."#,
    );

    test!(
        test_interpreter_call_export__invalid_types_in_the_stack =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallExport { export_name: "sum" },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I64(4),
                //              ^^^ mismatch with `sum` signature
            ],
            instance: Instance::new(),
            error: r#"`call-export "sum"` cannot call the exported function `sum` because the value types on the stack mismatch the function signature (expects [I32, I32])."#,
    );

    test!(
        test_interpreter_call_export__failure_when_calling =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallExport { export_name: "sum" },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance {
                exports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        "sum".into(),
                        Export {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |_| Err(()),
                            //            ^^^^^^^ function fails
                        },
                    );

                    hashmap
                },
                ..Default::default()
            },
            error: r#"`call-export "sum"` failed when calling the exported function `sum`."#,
    );

    test!(
        test_interpreter_call_export__void =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallExport { export_name: "sum" },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance {
                exports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        "sum".into(),
                        Export {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |_| Ok(vec![]),
                            //            ^^^^^^^^^^ void function
                        },
                    );

                    hashmap
                },
                ..Default::default()
            },
            stack: [],
    );

    test!(
        test_interpreter_read_utf8 =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::ReadUtf8,
            ],
            invocation_inputs: [
                InterfaceValue::I32(13),
                //              ^^^^^^^ length
                InterfaceValue::I32(0),
                //              ^^^^^^ pointer
            ],
            instance: Instance {
                memory: Memory::new("Hello, World!".as_bytes().iter().map(|u| Cell::new(*u)).collect()),
                ..Default::default()
            },
            stack: [InterfaceValue::String("Hello, World!".into())],
    );

    test!(
        test_interpreter_read_utf8__read_out_of_memory =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::ReadUtf8,
            ],
            invocation_inputs: [
                InterfaceValue::I32(13),
                //              ^^^^^^^ length is too long
                InterfaceValue::I32(0),
                //              ^^^^^^ pointer
            ],
            instance: Instance {
                memory: Memory::new("Hello!".as_bytes().iter().map(|u| Cell::new(*u)).collect()),
                ..Default::default()
            },
            error: r#"`read-utf8` failed because it has to read out of the memory bounds (index 13 > memory length 6)."#,
    );

    test!(
        test_interpreter_read_utf8__invalid_encoding =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::ReadUtf8,
            ],
            invocation_inputs: [
                InterfaceValue::I32(4),
                //              ^^^^^^ length is too long
                InterfaceValue::I32(0),
                //              ^^^^^^ pointer
            ],
            instance: Instance {
                memory: Memory::new(vec![0, 159, 146, 150].iter().map(|b| Cell::new(*b)).collect::<Vec<Cell<u8>>>()),
                ..Default::default()
            },
            error: r#"`read-utf8` failed because the read string isn't UTF-8 valid (invalid utf-8 sequence of 1 bytes from index 1)."#,
    );

    test!(
        test_interpreter_read_utf8__stack_is_too_small =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ReadUtf8,
                //           ^^^^^^^^ `read-utf8` expects 2 values on the stack, only one is present.
            ],
            invocation_inputs: [
                InterfaceValue::I32(13),
                InterfaceValue::I32(0),
            ],
            instance: Instance::new(),
            error: r#"`read-utf8` failed because there is no enough data on the stack (needs 2)."#,
    );

    test!(
        test_interpreter_call =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::Call { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(12)],
    );

    test!(
        test_interpreter_call__invalid_local_import_index =
            instructions: [
                Instruction::Call { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance { ..Default::default() },
            error: r#"`call 42` cannot call the local or imported function `42` because it doesn't exist."#,
    );

    test!(
        test_interpreter_call__stack_is_too_small =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::Call { function_index: 42 },
                //                                  ^^ `42` expects 2 values on the stack, only one is present
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance::new(),
            error: r#"`call 42` cannot call the local or imported function `42` because there is no enough data on the stack for the arguments (needs 2)."#,
    );

    test!(
        test_interpreter_call__invalid_types_in_the_stack =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::Call { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I64(4),
                //              ^^^ mismatch with `42` signature
            ],
            instance: Instance::new(),
            error: r#"`call 42` cannot call the local or imported function `42` because the value types on the stack mismatch the function signature (expects [I32, I32])."#,
    );

    test!(
        test_interpreter_call__failure_when_calling =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::Call { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance {
                locals_or_imports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        42,
                        LocalImport {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |_| Err(()),
                            //            ^^^^^^^ function fails
                        },
                    );

                    hashmap
                },
                ..Default::default()
            },
            error: r#"`call 42` failed when calling the local or imported function `42`."#,
    );

    test!(
        test_interpreter_call__void =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::Call { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance {
                locals_or_imports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        42,
                        LocalImport {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |_| Ok(vec![]),
                            //            ^^^^^^^^^^ void fails
                        },
                    );

                    hashmap
                },
                ..Default::default()
            },
            stack: [],
    );
}
