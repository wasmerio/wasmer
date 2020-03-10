use crate::interpreter::wasm::{
    structures::{FunctionIndex, TypedIndex},
    values::{InterfaceType, InterfaceValue},
};
use std::convert::TryInto;

executable_instruction!(
    string_to_memory(allocator_index: u32, instruction_name: String) -> _ {
        move |runtime| -> _ {
            let instance = &mut runtime.wasm_instance;
            let index = FunctionIndex::new(allocator_index as usize);

            let allocator = instance.local_or_import(index).ok_or_else(|| {
                format!(
                    "`{}` failed because the function `{}` (the allocator) doesn't exist.",
                    instruction_name,
                    allocator_index
                )
            })?;

            if allocator.inputs() != [InterfaceType::I32] || allocator.outputs() != [InterfaceType::I32] {
                return Err(format!(
                    "`{}` failed because the allocator `{}` has an invalid signature (expects [I32] -> [I32]).",
                    instruction_name,
                    allocator_index,
                ));
            }

            let string = runtime.stack.pop1().ok_or_else(|| {
                format!(
                    "`{}` cannot call the allocator `{}` because there is not enough data on the stack for the arguments (needs {}).",
                    instruction_name,
                    allocator_index,
                    1
                )
            })?;

            let string: String = (&string).try_into()?;
            let string_bytes = string.as_bytes();
            let string_length = (string_bytes.len() as i32)
                .try_into()
                .map_err(|error| format!("{}", error))?;

            let outputs = allocator.call(&[InterfaceValue::I32(string_length)]).map_err(|_| format!(
                    "`{}` failed when calling the allocator `{}`.",
                    instruction_name,
                    allocator_index,
            ))?;

            let string_pointer: i32 = (&outputs[0]).try_into()?;

            let memory_view = instance
                .memory(0)
                .ok_or_else(|| {
                    format!(
                        "`{}` failed because there is no memory to write into.",
                        instruction_name
                    )
                })?
                .view();

            for (nth, byte) in string_bytes.iter().enumerate() {
                memory_view[string_pointer as usize + nth].set(*byte);
            }

            runtime.stack.push(InterfaceValue::I32(string_pointer));
            runtime.stack.push(InterfaceValue::I32(string_length));

            Ok(())
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_string_to_memory =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringToMemory { allocator_index: 43 },
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            stack: [
                InterfaceValue::I32(0),
                //              ^^^^^^ pointer
                InterfaceValue::I32(13),
                //              ^^^^^^^ length
            ]
    );

    test_executable_instruction!(
        test_string_to_memory__roundtrip_with_memory_to_string =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringToMemory { allocator_index: 43 },
                Instruction::MemoryToString,
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            stack: [InterfaceValue::String("Hello, World!".into())],
    );

    test_executable_instruction!(
        test_string_to_memory__allocator_does_not_exist =
            instructions: [Instruction::StringToMemory { allocator_index: 43 }],
            invocation_inputs: [],
            instance: Instance { ..Default::default() },
            error: r#"`string-to-memory 43` failed because the function `43` (the allocator) doesn't exist."#,
    );

    test_executable_instruction!(
        test_string_to_memory__stack_is_too_small =
            instructions: [
                Instruction::StringToMemory { allocator_index: 43 }
                //                                             ^^ `43` expects 1 value on the stack, none is present
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            error: r#"`string-to-memory 43` cannot call the allocator `43` because there is not enough data on the stack for the arguments (needs 1)."#,
    );

    test_executable_instruction!(
        test_string_to_memory__failure_when_calling_the_allocator =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringToMemory { allocator_index: 153 }
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: {
                let mut instance = Instance::new();
                instance.locals_or_imports.insert(
                    153,
                    LocalImport {
                        inputs: vec![InterfaceType::I32],
                        outputs: vec![InterfaceType::I32],
                        function: |_| Err(()),
                        //            ^^^^^^^ function fails
                    },
                );

                instance
            },
            error: r#"`string-to-memory 153` failed when calling the allocator `153`."#,
    );

    test_executable_instruction!(
        test_string_to_memory__invalid_allocator_signature =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringToMemory { allocator_index: 153 }
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: {
                let mut instance = Instance::new();
                instance.locals_or_imports.insert(
                    153,
                    LocalImport {
                        inputs: vec![InterfaceType::I32, InterfaceType::I32],
                        outputs: vec![],
                        function: |_| Err(()),
                    },
                );

                instance
            },
            error: r#"`string-to-memory 153` failed because the allocator `153` has an invalid signature (expects [I32] -> [I32])."#,
    );
}
