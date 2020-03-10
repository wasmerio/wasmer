use super::to_native;
use crate::{
    errors::{InstructionError, InstructionErrorKind},
    interpreter::{wasm::values::InterfaceValue, Instruction},
};
use std::cell::Cell;

executable_instruction!(
    memory_to_string(instruction: Instruction) -> _ {
        move |runtime| -> _ {
            let inputs = runtime.stack.pop(2).ok_or_else(|| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::StackIsTooSmall { needed: 2 },
                )
            })?;

            let memory_index: u32 = 0;

            let memory = runtime
                .wasm_instance
                .memory(memory_index as usize)
                .ok_or_else(|| {
                    InstructionError::new(
                        instruction,
                        InstructionErrorKind::MemoryIsMissing { memory_index },
                    )
                })?;

            let length = to_native::<i32>(&inputs[0], instruction)? as usize;
            let pointer = to_native::<i32>(&inputs[1], instruction)? as usize;
            let memory_view = memory.view();

            if memory_view.len() < pointer + length {
                return Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::MemoryOutOfBoundsAccess {
                        index: pointer + length,
                        length: memory_view.len(),
                    },
                ));
            }

            let data: Vec<u8> = (&memory_view[pointer..pointer + length])
                .iter()
                .map(Cell::get)
                .collect();

            let string = String::from_utf8(data)
                .map_err(|error| InstructionError::new(instruction, InstructionErrorKind::String(error)))?;

            runtime.stack.push(InterfaceValue::String(string));

            Ok(())
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_memory_to_string =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::MemoryToString,
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
        test_memory_to_string__read_out_of_memory =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::MemoryToString,
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
            error: r#"`memory-to-string` read out of the memory bounds (index 13 > memory length 6)"#,
    );

    test_executable_instruction!(
        test_memory_to_string__invalid_encoding =
            instructions: [
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::MemoryToString,
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
            error: r#"`memory-to-string` invalid utf-8 sequence of 1 bytes from index 1"#,
    );

    test_executable_instruction!(
        test_memory_to_string__stack_is_too_small =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::MemoryToString,
                //           ^^^^^^^^^^^^^^ `memory-to-string` expects 2 values on the stack, only one is present.
            ],
            invocation_inputs: [
                InterfaceValue::I32(13),
                InterfaceValue::I32(0),
            ],
            instance: Instance::new(),
            error: r#"`memory-to-string` needed to read `2` value(s) from the stack, but it doesn't contain enough data"#,
    );
}
