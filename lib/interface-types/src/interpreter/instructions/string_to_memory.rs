use super::to_native;
use crate::{
    ast::InterfaceType,
    errors::{InstructionError, InstructionErrorKind},
    interpreter::{
        wasm::{
            structures::{FunctionIndex, TypedIndex},
            values::InterfaceValue,
        },
        Instruction,
    },
};

executable_instruction!(
    string_to_memory(allocator_index: u32, instruction: Instruction) -> _ {
        move |runtime| -> _ {
            let instance = &mut runtime.wasm_instance;
            let index = FunctionIndex::new(allocator_index as usize);

            let allocator = instance.local_or_import(index).ok_or_else(|| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::LocalOrImportIsMissing { function_index: allocator_index },
                )
            })?;

            if allocator.inputs() != [InterfaceType::I32] || allocator.outputs() != [InterfaceType::I32] {
                return Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::LocalOrImportSignatureMismatch {
                        function_index: allocator_index,
                        expected: (vec![InterfaceType::I32], vec![InterfaceType::I32]),
                        received: (allocator.inputs().to_vec(), allocator.outputs().to_vec())
                    }
                ))
            }

            let string = runtime.stack.pop1().ok_or_else(|| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::StackIsTooSmall { needed: 1 }
                )
            })?;

            let string: String = to_native(&string, instruction)?;
            let string_bytes = string.as_bytes();
            let string_length = string_bytes.len() as i32;

            let outputs = allocator.call(&[InterfaceValue::I32(string_length)]).map_err(|_| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::LocalOrImportCall { function_index: allocator_index },
                )
            })?;
            let string_pointer: i32 = to_native(&outputs[0], instruction)?;

            let memory_index: u32 = 0;
            let memory_view = instance
                .memory(memory_index as usize)
                .ok_or_else(|| {
                    InstructionError::new(
                        instruction,
                        InstructionErrorKind::MemoryIsMissing { memory_index }
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
            error: r#"`string-to-memory 43` the local or import function `43` doesn't exist"#,
    );

    test_executable_instruction!(
        test_string_to_memory__stack_is_too_small =
            instructions: [
                Instruction::StringToMemory { allocator_index: 43 }
                //                                             ^^ `43` expects 1 value on the stack, none is present
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            error: r#"`string-to-memory 43` needed to read `1` value(s) from the stack, but it doesn't contain enough data"#,
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
            error: r#"`string-to-memory 153` failed while calling the local or import function `153`"#,
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
            error: r#"`string-to-memory 153` the local or import function `153` has the signature `[I32] -> [I32]` but it received values of kind `[I32, I32] -> []`"#,
    );
}
