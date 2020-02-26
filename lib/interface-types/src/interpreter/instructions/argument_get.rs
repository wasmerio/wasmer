executable_instruction!(
    argument_get(index: u32, instruction_name: String) -> _ {
        move |runtime| -> _ {
            let invocation_inputs = runtime.invocation_inputs;

            if index >= (invocation_inputs.len() as u32) {
                return Err(format!(
                    "`{}` cannot access argument #{} because it doesn't exist.",
                    instruction_name, index
                ));
            }

            runtime.stack.push(invocation_inputs[index as usize].clone());

            Ok(())
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_argument_get =
            instructions: [Instruction::ArgumentGet { index: 0 }],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_argument_get__twice =
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

    test_executable_instruction!(
        test_argument_get__invalid_index =
            instructions: [Instruction::ArgumentGet { index: 1 }],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            error: "`arg.get 1` cannot access argument #1 because it doesn't exist."
    );
}
