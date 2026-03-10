use crate::interpreter::wasm::values::{InterfaceType, InterfaceValue};
use std::convert::TryInto;

executable_instruction!(
    write_utf8(allocator_name: String, instruction_name: String) -> _ {
        move |runtime| -> _ {
            let instance = &mut runtime.wasm_instance;

            match instance.export(&allocator_name) {
                Some(allocator) => {
                    if allocator.inputs() != [InterfaceType::I32] ||
                        allocator.outputs() != [InterfaceType::I32] {
                            return Err(format!(
                                "`{}` failed because the allocator `{}` has an invalid signature (expects [I32] -> [I32]).",
                                instruction_name,
                                allocator_name,
                            ))
                        }

                    match instance.memory(0) {
                        Some(memory) => match runtime.stack.pop1() {
                            Some(string) => {
                                let memory_view = memory.view();

                                let string: String = (&string).try_into()?;
                                let string_bytes = string.as_bytes();
                                let string_length = (string_bytes.len() as i32)
                                    .try_into()
                                    .map_err(|error| format!("{}", error))?;

                                match allocator.call(&[InterfaceValue::I32(string_length)]) {
                                    Ok(outputs) => {
                                        let string_pointer: i32 = (&outputs[0]).try_into()?;

                                        for (nth, byte) in string_bytes.iter().enumerate() {
                                            memory_view[string_pointer as usize + nth].set(*byte);
                                        }

                                        runtime.stack.push(InterfaceValue::I32(string_pointer));
                                        runtime.stack.push(InterfaceValue::I32(string_length));

                                        Ok(())
                                    }
                                    Err(_) => Err(format!(
                                        "`{}` failed when calling the allocator `{}`.",
                                        instruction_name,
                                        allocator_name,
                                    ))
                                }
                            }
                            None => Err(format!(
                                "`{}` cannot call the allocator `{}` because there is not enough data on the stack for the arguments (needs {}).",
                                instruction_name,
                                allocator_name,
                                1
                            ))
                        }
                        None => Err(format!(
                            "`{}` failed because there is no memory to write into.",
                            instruction_name
                        ))
                    }
                }
                None => Err(format!(
                    "`{}` failed because the exported function `{}` (the allocator) doesn't exist.",
                    instruction_name,
                    allocator_name
                ))
            }
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_write_utf8 =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::WriteUtf8 { allocator_name: "alloc" },
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
        test_write_utf8__roundtrip_with_read_utf8 =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::WriteUtf8 { allocator_name: "alloc" },
                Instruction::ReadUtf8,
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            stack: [InterfaceValue::String("Hello, World!".into())],
    );

    test_executable_instruction!(
        test_write_utf8__allocator_does_not_exist =
            instructions: [Instruction::WriteUtf8 { allocator_name: "alloc" }],
            invocation_inputs: [],
            instance: Instance { ..Default::default() },
            error: r#"`write-utf8 "alloc"` failed because the exported function `alloc` (the allocator) doesn't exist."#,
    );

    test_executable_instruction!(
        test_write_utf8__stack_is_too_small =
            instructions: [
                Instruction::WriteUtf8 { allocator_name: "alloc" }
                //                                        ^^^^^ `alloc` expects 1 value on the stack, none is present
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            error: r#"`write-utf8 "alloc"` cannot call the allocator `alloc` because there is not enough data on the stack for the arguments (needs 1)."#,
    );

    test_executable_instruction!(
        test_write_utf8__failure_when_calling_the_allocator =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::WriteUtf8 { allocator_name: "alloc-fail" }
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: {
                let mut instance = Instance::new();
                instance.exports.insert(
                    "alloc-fail".into(),
                    Export {
                        inputs: vec![InterfaceType::I32],
                        outputs: vec![InterfaceType::I32],
                        function: |_| Err(()),
                        //            ^^^^^^^ function fails
                    },
                );

                instance
            },
            error: r#"`write-utf8 "alloc-fail"` failed when calling the allocator `alloc-fail`."#,
    );

    test_executable_instruction!(
        test_write_utf8__invalid_allocator_signature =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::WriteUtf8 { allocator_name: "alloc-fail" }
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: {
                let mut instance = Instance::new();
                instance.exports.insert(
                    "alloc-fail".into(),
                    Export {
                        inputs: vec![InterfaceType::I32, InterfaceType::I32],
                        outputs: vec![],
                        function: |_| Err(()),
                    },
                );

                instance
            },
            error: r#"`write-utf8 "alloc-fail"` failed because the allocator `alloc-fail` has an invalid signature (expects [I32] -> [I32])."#,
    );
}
