use crate::interpreter::wasm::values::InterfaceValue;
use std::{cell::Cell, convert::TryFrom};

executable_instruction!(
    read_utf8(instruction_name: String) -> _ {
        move |runtime| -> _ {
            match runtime.stack.pop(2) {
                Some(inputs) => match runtime.wasm_instance.memory(0) {
                    Some(memory) => {
                        let length = i32::try_from(&inputs[0])? as usize;
                        let pointer = i32::try_from(&inputs[1])? as usize;
                        let memory_view = memory.view();

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
                    "`{}` failed because there is not enough data on the stack (needs 2).",
                    instruction_name,
                ))
            }
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_read_utf8 =
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

    test_executable_instruction!(
        test_read_utf8__read_out_of_memory =
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

    test_executable_instruction!(
        test_read_utf8__invalid_encoding =
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

    test_executable_instruction!(
        test_read_utf8__stack_is_too_small =
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
            error: r#"`read-utf8` failed because there is not enough data on the stack (needs 2)."#,
    );
}
