use crate::interpreter::wasm::values::InterfaceValue;
use std::convert::TryInto;

macro_rules! lowering_lifting {
    ($instruction_function_name:ident, $instruction_name:expr, $from_variant:ident, $to_variant:ident) => {
        executable_instruction!(
            $instruction_function_name() -> _ {
                move |runtime| -> _ {
                    match runtime.stack.pop1() {
                        Some(InterfaceValue::$from_variant(value)) => {
                            runtime
                                .stack
                                .push(InterfaceValue::$to_variant(value.try_into().map_err(
                                    |_| {
                                        concat!(
                                            "Failed to cast `",
                                            stringify!($from_variant),
                                            "` to `",
                                            stringify!($to_variant),
                                            "`."
                                        ).to_string()
                                    },
                                )?))
                        }

                        Some(wrong_value) => {
                            return Err(format!(
                                concat!(
                                    "Instruction `",
                                    $instruction_name,
                                    "` expects a `",
                                    stringify!($from_variant),
                                    "` value on the stack, got `{:?}`.",
                                ),
                                wrong_value

                            )
                            .to_string())
                        },

                        None => {
                            return Err(concat!(
                                "Instruction `",
                                $instruction_name,
                                "` needs one value on the stack."
                            )
                            .to_string())
                        }
                    }

                    Ok(())
                }
            }
        );
    };
}

lowering_lifting!(i32_to_s8, "i32-to-s8", I32, S8);
lowering_lifting!(i32_to_u8, "i32-to-u8", I32, U8);
lowering_lifting!(i32_to_s16, "i32-to-s16", I32, S16);
lowering_lifting!(i32_to_u16, "i32-to-u16", I32, U16);
lowering_lifting!(i32_to_s32, "i32-to-s32", I32, S32);
lowering_lifting!(i32_to_u32, "i32-to-u32", I32, U32);
lowering_lifting!(i32_to_s64, "i32-to-s64", I32, S64);
lowering_lifting!(i32_to_u64, "i32-to-u64", I32, U64);
lowering_lifting!(i64_to_s8, "i64-to-s8", I64, S8);
lowering_lifting!(i64_to_u8, "i64-to-u8", I64, U8);
lowering_lifting!(i64_to_s16, "i64-to-s16", I64, S16);
lowering_lifting!(i64_to_u16, "i64-to-u16", I64, U16);
lowering_lifting!(i64_to_s32, "i64-to-s32", I64, S32);
lowering_lifting!(i64_to_u32, "i64-to-u32", I64, U32);
lowering_lifting!(i64_to_s64, "i64-to-s64", I64, S64);
lowering_lifting!(i64_to_u64, "i64-to-u64", I64, U64);
lowering_lifting!(s8_to_i32, "s8-to-i32", S8, I32);
lowering_lifting!(u8_to_i32, "u8-to-i32", U8, I32);
lowering_lifting!(s16_to_i32, "s16-to-i32", S16, I32);
lowering_lifting!(u16_to_i32, "u16-to-i32", U16, I32);
lowering_lifting!(s32_to_i32, "s32-to-i32", S32, I32);
lowering_lifting!(u32_to_i32, "u32-to-i32", U32, I32);
lowering_lifting!(s64_to_i32, "s64-to-i32", S64, I32);
lowering_lifting!(u64_to_i32, "u64-to-i32", U64, I32);
lowering_lifting!(s8_to_i64, "s8-to-i64", S8, I64);
lowering_lifting!(u8_to_i64, "u8-to-i64", U8, I64);
lowering_lifting!(s16_to_i64, "s16-to-i64", S16, I64);
lowering_lifting!(u16_to_i64, "u16-to-i64", U16, I64);
lowering_lifting!(s32_to_i64, "s32-to-i64", S32, I64);
lowering_lifting!(u32_to_i64, "u32-to-i64", U32, I64);
lowering_lifting!(s64_to_i64, "s64-to-i64", S64, I64);
lowering_lifting!(u64_to_i64, "u64-to-i64", U64, I64);

#[cfg(test)]
mod tests {
    test_executable_instruction!(
        test_i32_to_s8 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToS8],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S8(42)],
    );

    test_executable_instruction!(
        test_convert_fails =
            instructions: [Instruction::ArgumentGet { index: 0}, Instruction::I32ToS8],
            invocation_inputs: [InterfaceValue::I32(128)],
            instance: Instance::new(),
            error: "Failed to cast `I32` to `S8`."
    );

    test_executable_instruction!(
        test_type_mismatch =
            instructions: [Instruction::ArgumentGet { index: 0}, Instruction::I32ToS8],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            error: "Instruction `i32-to-s8` expects a `I32` value on the stack, got `I64(42)`."
    );

    test_executable_instruction!(
        test_no_value_on_the_stack =
            instructions: [Instruction::I32ToS8],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            error: "Instruction `i32-to-s8` needs one value on the stack."
    );

    test_executable_instruction!(
        test_i32_to_u8 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToU8],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U8(42)],
    );

    test_executable_instruction!(
        test_i32_to_s16 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToS16],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S16(42)],
    );

    test_executable_instruction!(
        test_i32_to_u16 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToU16],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U16(42)],
    );

    test_executable_instruction!(
        test_i32_to_s32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToS32],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S32(42)],
    );

    test_executable_instruction!(
        test_i32_to_u32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToU32],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U32(42)],
    );

    test_executable_instruction!(
        test_i32_to_s64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToS64],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S64(42)],
    );

    test_executable_instruction!(
        test_i32_to_u64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I32ToU64],
            invocation_inputs: [InterfaceValue::I32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U64(42)],
    );

    test_executable_instruction!(
        test_i64_to_s8 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToS8],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S8(42)],
    );

    test_executable_instruction!(
        test_i64_to_u8 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToU8],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U8(42)],
    );

    test_executable_instruction!(
        test_i64_to_s16 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToS16],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S16(42)],
    );

    test_executable_instruction!(
        test_i64_to_u16 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToU16],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U16(42)],
    );

    test_executable_instruction!(
        test_i64_to_s32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToS32],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S32(42)],
    );

    test_executable_instruction!(
        test_i64_to_u32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToU32],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U32(42)],
    );

    test_executable_instruction!(
        test_i64_to_s64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToS64],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::S64(42)],
    );

    test_executable_instruction!(
        test_i64_to_u64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::I64ToU64],
            invocation_inputs: [InterfaceValue::I64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::U64(42)],
    );

    test_executable_instruction!(
        test_s8_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S8ToI32],
            invocation_inputs: [InterfaceValue::S8(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_u8_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U8ToI32],
            invocation_inputs: [InterfaceValue::U8(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_s16_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S16ToI32],
            invocation_inputs: [InterfaceValue::S16(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_u16_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U16ToI32],
            invocation_inputs: [InterfaceValue::U16(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_s32_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S32ToI32],
            invocation_inputs: [InterfaceValue::S32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_u32_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U32ToI32],
            invocation_inputs: [InterfaceValue::U32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_s64_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S64ToI32],
            invocation_inputs: [InterfaceValue::S64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_u64_to_i32 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U64ToI32],
            invocation_inputs: [InterfaceValue::U64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I32(42)],
    );

    test_executable_instruction!(
        test_s8_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S8ToI64],
            invocation_inputs: [InterfaceValue::S8(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );

    test_executable_instruction!(
        test_u8_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U8ToI64],
            invocation_inputs: [InterfaceValue::U8(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );

    test_executable_instruction!(
        test_s16_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S16ToI64],
            invocation_inputs: [InterfaceValue::S16(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );

    test_executable_instruction!(
        test_u16_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U16ToI64],
            invocation_inputs: [InterfaceValue::U16(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );

    test_executable_instruction!(
        test_s32_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S32ToI64],
            invocation_inputs: [InterfaceValue::S32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );

    test_executable_instruction!(
        test_u32_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U32ToI64],
            invocation_inputs: [InterfaceValue::U32(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );

    test_executable_instruction!(
        test_s64_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::S64ToI64],
            invocation_inputs: [InterfaceValue::S64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );

    test_executable_instruction!(
        test_u64_to_i64 =
            instructions: [Instruction::ArgumentGet { index: 0 }, Instruction::U64ToI64],
            invocation_inputs: [InterfaceValue::U64(42)],
            instance: Instance::new(),
            stack: [InterfaceValue::I64(42)],
    );
}
