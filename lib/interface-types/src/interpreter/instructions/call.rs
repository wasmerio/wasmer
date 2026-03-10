use crate::interpreter::wasm::{
    structures::{FunctionIndex, TypedIndex},
    values::InterfaceType,
};

executable_instruction!(
    call(function_index: usize, instruction_name: String) -> _ {
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
                                return Err(format!(
                                    "`{}` cannot call the local or imported function `{}` because the value types on the stack mismatch the function signature (expects {:?}).",
                                    instruction_name,
                                    function_index,
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
                                    function_index
                                ))
                            }
                        }
                        None => Err(format!(
                            "`{}` cannot call the local or imported function `{}` because there is not enough data on the stack for the arguments (needs {}).",
                            instruction_name,
                            function_index,
                            inputs_cardinality,
                        ))
                    }
                }
                None => Err(format!(
                    "`{}` cannot call the local or imported function `{}` because it doesn't exist.",
                    instruction_name,
                    function_index,
                ))
            }
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_call =
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

    test_executable_instruction!(
        test_call__invalid_local_import_index =
            instructions: [
                Instruction::Call { function_index: 42 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Default::default(),
            error: r#"`call 42` cannot call the local or imported function `42` because it doesn't exist."#,
    );

    test_executable_instruction!(
        test_call__stack_is_too_small =
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
            error: r#"`call 42` cannot call the local or imported function `42` because there is not enough data on the stack for the arguments (needs 2)."#,
    );

    test_executable_instruction!(
        test_call__invalid_types_in_the_stack =
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

    test_executable_instruction!(
        test_call__failure_when_calling =
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

    test_executable_instruction!(
        test_call__void =
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
