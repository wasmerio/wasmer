use super::to_native;
use crate::{
    errors::{InstructionError, InstructionErrorKind},
    interpreter::{wasm::values::InterfaceValue, Instruction},
    types::InterfaceType,
};
use std::{cell::Cell, convert::TryInto};

executable_instruction!(
    string_lift_memory(instruction: Instruction) -> _ {
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

            let pointer: usize = to_native::<i32>(&inputs[0], instruction)?
                .try_into()
                .map_err(|e| (e, "pointer").into())
                .map_err(|k| InstructionError::new(instruction, k))?;
            let length: usize = to_native::<i32>(&inputs[1], instruction)?
                .try_into()
                .map_err(|e| (e, "length").into())
                .map_err(|k| InstructionError::new(instruction, k))?;
            let memory_view = memory.view();

            if length == 0 {
                runtime.stack.push(InterfaceValue::String("".into()));

                return Ok(())
            }

            if memory_view.len() <= pointer + length - 1 {
                return Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::MemoryOutOfBoundsAccess {
                        index: pointer + length,
                        length: memory_view.len(),
                    },
                ));
            }

            let data: Vec<u8> = (&memory_view[pointer..=pointer + length - 1])
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

executable_instruction!(
    string_lower_memory(instruction: Instruction) -> _ {
        move |runtime| -> _ {
            let inputs = runtime.stack.pop(2).ok_or_else(|| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::StackIsTooSmall { needed: 2 },
                )
            })?;

            let string_pointer: usize = to_native::<i32>(&inputs[0], instruction)?
                .try_into()
                .map_err(|e| (e, "pointer").into())
                .map_err(|k| InstructionError::new(instruction, k))?;
            let string: String = to_native(&inputs[1], instruction)?;
            let string_bytes = string.as_bytes();
            let string_length: i32 = string_bytes.len().try_into().map_err(|_| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::NegativeValue { subject: "string_length" },
                )
            })?;

            let instance = &mut runtime.wasm_instance;
            let memory_index: u32 = 0;
            let memory_view = instance
                .memory(memory_index as usize)
                .ok_or_else(|| {
                    InstructionError::new(
                        instruction,
                        InstructionErrorKind::MemoryIsMissing { memory_index },
                    )
                })?
                .view();

            for (nth, byte) in string_bytes.iter().enumerate() {
                memory_view[string_pointer as usize + nth].set(*byte);
            }

            runtime.stack.push(InterfaceValue::I32(string_pointer as i32));
            runtime.stack.push(InterfaceValue::I32(string_length));

            Ok(())
        }
    }
);

executable_instruction!(
    string_size(instruction: Instruction) -> _ {
        move |runtime| -> _ {
            match runtime.stack.pop1() {
                Some(InterfaceValue::String(string)) => {
                    let length = string.len() as i32;
                    runtime.stack.push(InterfaceValue::I32(length));

                    Ok(())
                },

                Some(value) => Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::InvalidValueOnTheStack {
                        expected_type: InterfaceType::String,
                        received_type: (&value).into(),
                    },
                )),

                None => Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::StackIsTooSmall { needed: 1 },
                )),
            }
        }
    }
);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_string_lift_memory =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::StringLiftMemory,
            ],
            invocation_inputs: [
                InterfaceValue::I32(0),
                //              ^^^^^^ pointer
                InterfaceValue::I32(13),
                //              ^^^^^^^ length
            ],
            instance: Instance {
                memory: Memory::new("Hello, World!".as_bytes().iter().map(|u| Cell::new(*u)).collect()),
                ..Default::default()
            },
            stack: [InterfaceValue::String("Hello, World!".into())],
    );

    test_executable_instruction!(
        test_string_lift_memory__empty_string =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::StringLiftMemory,
            ],
            invocation_inputs: [
                InterfaceValue::I32(0),
                InterfaceValue::I32(0),
            ],
            instance: Instance {
                memory: Memory::new(vec![]),
                ..Default::default()
            },
            stack: [InterfaceValue::String("".into())],
    );

    test_executable_instruction!(
        test_string_lift_memory__negative_pointer =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::StringLiftMemory,
            ],
            invocation_inputs: [
                InterfaceValue::I32(-42),
                InterfaceValue::I32(13),
            ],
            instance: Instance {
                memory: Memory::new("Hello!".as_bytes().iter().map(|u| Cell::new(*u)).collect()),
                ..Default::default()
            },
            error: r#"`string.lift_memory` attempted to convert `pointer` but it appears to be a negative value"#,
    );

    test_executable_instruction!(
        test_string_lift_memory__negative_length =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::StringLiftMemory,
            ],
            invocation_inputs: [
                InterfaceValue::I32(0),
                InterfaceValue::I32(-1),
            ],
            instance: Instance {
                memory: Memory::new("Hello!".as_bytes().iter().map(|u| Cell::new(*u)).collect()),
                ..Default::default()
            },
            error: r#"`string.lift_memory` attempted to convert `length` but it appears to be a negative value"#,
    );

    test_executable_instruction!(
        test_string_lift_memory__read_out_of_memory =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::StringLiftMemory,
            ],
            invocation_inputs: [
                InterfaceValue::I32(0),
                //              ^^^^^^ pointer
                InterfaceValue::I32(13),
                //              ^^^^^^^ length is too long
            ],
            instance: Instance {
                memory: Memory::new("Hello!".as_bytes().iter().map(|u| Cell::new(*u)).collect()),
                ..Default::default()
            },
            error: r#"`string.lift_memory` read out of the memory bounds (index 13 > memory length 6)"#,
    );

    test_executable_instruction!(
        test_string_lift_memory__invalid_encoding =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::StringLiftMemory,
            ],
            invocation_inputs: [
                InterfaceValue::I32(0),
                //              ^^^^^^ pointer
                InterfaceValue::I32(4),
                //              ^^^^^^ length is too long
            ],
            instance: Instance {
                memory: Memory::new(vec![0, 159, 146, 150].iter().map(|b| Cell::new(*b)).collect::<Vec<Cell<u8>>>()),
                ..Default::default()
            },
            error: r#"`string.lift_memory` invalid utf-8 sequence of 1 bytes from index 1"#,
    );

    test_executable_instruction!(
        test_string_lift_memory__stack_is_too_small =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringLiftMemory,
                //           ^^^^^^^^^^^^^^^^ `string.lift_memory` expects 2 values on the stack, only one is present.
            ],
            invocation_inputs: [
                InterfaceValue::I32(0),
                InterfaceValue::I32(13),
            ],
            instance: Instance::new(),
            error: r#"`string.lift_memory` needed to read `2` value(s) from the stack, but it doesn't contain enough data"#,
    );

    test_executable_instruction!(
        test_string_lower_memory =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringSize,
                Instruction::CallCore { function_index: 43 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringLowerMemory,

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
        test_string__roundtrip =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringSize,
                Instruction::CallCore { function_index: 43 },
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringLowerMemory,
                Instruction::StringLiftMemory,
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            stack: [InterfaceValue::String("Hello, World!".into())],
    );

    test_executable_instruction!(
        test_string_lower_memory__stack_is_too_small =
            instructions: [
                Instruction::StringLowerMemory,
            ],
            invocation_inputs: [],
            instance: Instance::new(),
            error: r#"`string.lower_memory` needed to read `2` value(s) from the stack, but it doesn't contain enough data"#,
    );

    test_executable_instruction!(
        test_string_size =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringSize,
            ],
            invocation_inputs: [InterfaceValue::String("Hello, World!".into())],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(13)],
    );

    test_executable_instruction!(
        test_string_size__stack_is_too_small =
            instructions: [
                Instruction::StringSize,
            ],
            invocation_inputs: [],
            instance: Instance::new(),
            error: r#"`string.size` needed to read `1` value(s) from the stack, but it doesn't contain enough data"#,
    );

    test_executable_instruction!(
        test_string_size__invalid_value_on_the_stack =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::StringSize,
            ],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            error: r#"`string.size` read a value of type `I32` from the stack, but the type `String` was expected"#,
    );
}
