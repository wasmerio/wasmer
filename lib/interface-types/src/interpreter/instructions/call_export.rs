use crate::interpreter::wasm::values::InterfaceType;

executable_instruction!(
    call_export(export_name: String, instruction_name: String) -> _ {
        move |runtime| -> _ {
            let instance = &mut runtime.wasm_instance;

            match instance.export(&export_name) {
                Some(export) => {
                    let inputs_cardinality = export.inputs_cardinality();

                    match runtime.stack.pop(inputs_cardinality) {
                        Some(inputs) =>  {
                            let input_types = inputs
                                .iter()
                                .map(Into::into)
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
                            "`{}` cannot call the exported function `{}` because there is not enough data on the stack for the arguments (needs {}).",
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
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_call_export =
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

    test_executable_instruction!(
        test_call_export__invalid_export_name =
            instructions: [Instruction::CallExport { export_name: "bar" }],
            invocation_inputs: [],
            instance: Instance::new(),
            error: r#"`call-export "bar"` cannot call the exported function `bar` because it doesn't exist."#,
    );

    test_executable_instruction!(
        test_call_export__stack_is_too_small =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::CallExport { export_name: "sum" },
            ],
            invocation_inputs: [
                InterfaceValue::I32(3),
                InterfaceValue::I32(4),
            ],
            instance: Instance::new(),
            error: r#"`call-export "sum"` cannot call the exported function `sum` because there is not enough data on the stack for the arguments (needs 2)."#,
    );

    test_executable_instruction!(
        test_call_export__invalid_types_in_the_stack =
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

    test_executable_instruction!(
        test_call_export__failure_when_calling =
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

    test_executable_instruction!(
        test_call_export__void =
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
}
