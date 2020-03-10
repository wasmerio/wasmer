use crate::{
    errors::{InstructionError, InstructionErrorKind},
    interpreter::wasm::{
        structures::{FunctionIndex, TypedIndex},
        values::InterfaceType,
    },
    interpreter::Instruction,
};

executable_instruction!(
    call_core(function_index: usize, instruction: Instruction) -> _ {
        move |runtime| -> _ {
            let instance = &mut runtime.wasm_instance;
            let index = FunctionIndex::new(function_index);

            match instance.local_or_import(index) {
                Some(local_or_import) => {
                    let inputs_cardinality = local_or_import.inputs_cardinality();

                    match runtime.stack.pop(inputs_cardinality) {
                        Some(inputs) =>  {
                            let input_types = inputs
                                .iter()
                                .map(Into::into)
                                .collect::<Vec<InterfaceType>>();

                            if input_types != local_or_import.inputs() {
                                return Err(
                                    InstructionError::new(
                                        instruction,
                                        InstructionErrorKind::LocalOrImportSignatureMismatch {
                                            function_index: function_index as u32,
                                            expected: (local_or_import.inputs().to_vec(), vec![]),
                                            received: (input_types, vec![]),
                                        }
                                    )
                                )
                            }

                            match local_or_import.call(&inputs) {
                                Ok(outputs) => {
                                    for output in outputs.iter() {
                                        runtime.stack.push(output.clone());
                                    }

                                    Ok(())
                                }
                                Err(_) => Err(
                                    InstructionError::new(
                                        instruction,
                                        InstructionErrorKind::LocalOrImportCall { function_index: function_index as u32, },
                                    )
                                )
                            }
                        }
                        None => Err(
                            InstructionError::new(
                                instruction,
                                InstructionErrorKind::StackIsTooSmall { needed: inputs_cardinality },
                            )
                        )
                    }
                }
                None => Err(
                    InstructionError::new(
                        instruction,
                        InstructionErrorKind::LocalOrImportIsMissing { function_index: function_index as u32, },
                    )
                )
            }
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_call_core =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallCore { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(12)],
    );

    test_executable_instruction!(
        test_call_core__invalid_local_import_index =
            instructions: [
                Instruction::CallCore { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Default::default(),
            error: r#"`call-core 42` the local or import function `42` doesn't exist"#,
    );

    test_executable_instruction!(
        test_call_core__stack_is_too_small =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallCore { function_index: 42 },
                //                                      ^^ `42` expects 2 values on the stack, only one is present
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance::new(),
            error: r#"`call-core 42` needed to read `2` value(s) from the stack, but it doesn't contain enough data"#,
    );

    test_executable_instruction!(
        test_call_core__invalid_types_in_the_stack =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallCore { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I64(4),
                //              ^^^ mismatch with `42` signature
            ],
            instance: Instance::new(),
            error: r#"`call-core 42` the local or import function `42` has the signature `[I32, I32] -> []` but it received values of kind `[I32, I64] -> []`"#,
    );

    test_executable_instruction!(
        test_call_core__failure_when_calling =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallCore { function_index: 42 },
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
            error: r#"`call-core 42` failed while calling the local or import function `42`"#,
    );

    test_executable_instruction!(
        test_call_core__void =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallCore { function_index: 42 },
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
                            //            ^^^^^^^^^^ void
                        },
                    );

                    hashmap
                },
                ..Default::default()
            },
            stack: [],
    );
}
