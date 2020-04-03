use crate::{
    ast::{InterfaceType, RecordType, Type, TypeKind},
    errors::{InstructionError, InstructionErrorKind},
    interpreter::wasm::values::FlattenInterfaceValueIterator,
    interpreter::{
        stack::{Stack, Stackable},
        wasm::values::InterfaceValue,
        Instruction,
    },
};
use std::mem::{transmute, MaybeUninit};

/// Build a `InterfaceValue::Record` based on values on the stack.
///
/// To fill a record, every field `field_1` to `field_n` must get its
/// value from the stack with `value_1` to `value_n`. To simplify this
/// algorithm that also typed-checks values when hydrating, the number
/// of values to read from the stack isn't known ahead-of-time. Thus,
/// the `Stack::pop` method cannot be used, and `Stack::pop1` is used
/// instead. It implies that values are read one after the other from
/// the stack, in a natural reverse order, from `value_n` to
/// `value_1`.
///
/// Consequently, record fields are filled in reverse order, from
/// `field_n` to `field_1`.
///
/// A basic algorithm would then be:
///
/// ```rust,ignore
/// let mut values = vec![];
///
/// // Read fields in reverse-order, from `field_n` to `field_1`.
/// for field in fields.iter().rev() {
///     let value = stack.pop1();
///     // type-check with `field` and `value`, to finally…
///     values.push(value);
/// }
///
/// InterfaceValue::Record(values.iter().rev().collect())
/// ```
///
/// Note that it is required to reverse the `values` vector at the end
/// because `InterfaceValue::Record` expects its values to match the
/// original `fields` order.
///
/// Because this approach allocates two vectors for `values`, another
/// approach has been adopted. `values` is an initialized vector
/// containing uninitialized values of type
/// `MaybeUninit<InterfaceValue>`. With this approach, it is possible
/// to fill `values` from index `n` to `0`. Once `values` is entirely
/// filled, it is `transmute`d to `Vec<InterfaceType>`.
///
/// This latter approach allows to allocate one and final vector to
/// hold all the record values.
#[allow(unsafe_code)]
fn record_lift_(
    stack: &mut Stack<InterfaceValue>,
    record_type: &RecordType,
) -> Result<InterfaceValue, InstructionErrorKind> {
    let length = record_type.fields.len();
    let mut values = {
        // Initialize a vector of length `length` with `MaybeUninit`
        // values.
        let mut v = Vec::with_capacity(length);

        for _ in 0..length {
            v.push(MaybeUninit::<InterfaceValue>::uninit());
        }

        v
    };
    let max = length - 1;

    // Iterate over fields in reverse order to match the stack `pop`
    // order.
    for (nth, field) in record_type.fields.iter().rev().enumerate() {
        match field {
            // The record type tells a record is expected.
            InterfaceType::Record(record_type) => {
                // Build it recursively.
                let value = record_lift_(stack, &record_type)?;

                unsafe {
                    values[max - nth].as_mut_ptr().write(value);
                }
            }
            // Any other type.
            ty => {
                let value = stack.pop1().unwrap();
                let value_type = (&value).into();

                if ty != &value_type {
                    return Err(InstructionErrorKind::InvalidValueOnTheStack {
                        expected_type: ty.clone(),
                        received_type: value_type,
                    });
                }

                unsafe {
                    values[max - nth].as_mut_ptr().write(value);
                }
            }
        }
    }

    Ok(InterfaceValue::Record(unsafe { transmute(values) }))
}

executable_instruction!(
    record_lift(type_index: u32, instruction: Instruction) -> _ {
        move |runtime| -> _ {
            let instance = &runtime.wasm_instance;
            let record_type = match instance.wit_type(type_index).ok_or_else(|| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::TypeIsMissing { type_index },
                )
            })? {
                Type::Record(record_type) => record_type,
                Type::Function { .. } => return Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::InvalidTypeKind {
                        expected_kind: TypeKind::Record,
                        received_kind: TypeKind::Function
                    }
                )),
            };

            let record = record_lift_(&mut runtime.stack, &record_type)
                .map_err(|k| InstructionError::new(instruction, k))?;

            runtime.stack.push(record);

            Ok(())
        }
    }
);

executable_instruction!(
    record_lower(type_index: u32, instruction: Instruction) -> _ {
        move |runtime| -> _ {
            let instance = &runtime.wasm_instance;
            let record_type = match instance.wit_type(type_index).ok_or_else(|| {
                InstructionError::new(
                    instruction,
                    InstructionErrorKind::TypeIsMissing { type_index },
                )
            })? {
                Type::Record(record_type) => record_type,
                Type::Function { .. } => return Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::InvalidTypeKind {
                        expected_kind: TypeKind::Record,
                        received_kind: TypeKind::Function
                    }
                )),
            };

            match runtime.stack.pop1() {
                Some(InterfaceValue::Record(record_values)) if record_type == &(&record_values).into() => {
                    let values = FlattenInterfaceValueIterator::new(&record_values);

                    for value in values {
                        runtime.stack.push(value.clone());
                    }

                    Ok(())
                },

                Some(value) => Err(InstructionError::new(
                    instruction,
                    InstructionErrorKind::InvalidValueOnTheStack {
                        expected_type: InterfaceType::Record(record_type.clone()),
                        received_type: (&value).into(),
                    }
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
    use crate::ast::{RecordType, Type};

    test_executable_instruction!(
        test_record_lift =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 2 },
                Instruction::ArgumentGet { index: 3 },
                Instruction::RecordLift { type_index: 0 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(1),
                InterfaceValue::String("Hello".to_string()),
                InterfaceValue::F32(2.),
                InterfaceValue::I64(3),
            ],
            instance: Instance::new(),
            stack: [InterfaceValue::Record(vec![
                InterfaceValue::I32(1),
                InterfaceValue::Record(vec![
                    InterfaceValue::String("Hello".to_string()),
                    InterfaceValue::F32(2.),
                ]),
                InterfaceValue::I64(3),
            ])],
    );

    #[cfg(feature = "serde")]
    #[test]
    #[allow(non_snake_case, unused)]
    fn test_record_lift__to_rust_struct() {
        use crate::interpreter::{
            instructions::tests::{Export, Instance, LocalImport, Memory, MemoryView},
            stack::Stackable,
            wasm::values::{from_interface_values, InterfaceType, InterfaceValue},
            Instruction, Interpreter,
        };
        use serde::Deserialize;
        use std::{cell::Cell, collections::HashMap, convert::TryInto};

        let interpreter: Interpreter<Instance, Export, LocalImport, Memory, MemoryView> = (&vec![
            Instruction::ArgumentGet { index: 0 },
            Instruction::ArgumentGet { index: 1 },
            Instruction::ArgumentGet { index: 2 },
            Instruction::ArgumentGet { index: 3 },
            Instruction::RecordLift { type_index: 0 },
        ])
            .try_into()
            .unwrap();

        let invocation_inputs = vec![
            InterfaceValue::I32(1),
            InterfaceValue::String("Hello".to_string()),
            InterfaceValue::F32(2.),
            InterfaceValue::I64(3),
        ];
        let mut instance = Instance::new();
        let run = interpreter.run(&invocation_inputs, &mut instance);

        assert!(run.is_ok());

        let stack = run.unwrap();

        #[derive(Deserialize, Debug, PartialEq)]
        struct S {
            a: String,
            b: f32,
        }

        #[derive(Deserialize, Debug, PartialEq)]
        struct T {
            x: i32,
            s: S,
            y: i64,
        }

        let record: T = from_interface_values(stack.as_slice()).unwrap();

        assert_eq!(
            record,
            T {
                x: 1,
                s: S {
                    a: "Hello".to_string(),
                    b: 2.,
                },
                y: 3,
            }
        );
    }

    test_executable_instruction!(
        test_record_lift__one_dimension =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::RecordLift { type_index: 1 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(1),
                InterfaceValue::I32(2),
            ],
            instance: {
                let mut instance = Instance::new();
                instance.wit_types.push(
                    Type::Record(RecordType {
                        fields: vec1![InterfaceType::I32, InterfaceType::I32],
                    })
                );

                instance
            },
            stack: [InterfaceValue::Record(vec![
                InterfaceValue::I32(1),
                InterfaceValue::I32(2),
            ])],
    );

    test_executable_instruction!(
        test_record_lift__type_is_missing =
            instructions: [
                Instruction::RecordLift { type_index: 0 },
            ],
            invocation_inputs: [],
            instance: Default::default(),
            error: r#"`record.lift 0` the type `0` doesn't exist"#,
    );

    test_executable_instruction!(
        test_record_lift__invalid_value_on_the_stack =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::ArgumentGet { index: 1 },
                Instruction::ArgumentGet { index: 2 },
                Instruction::ArgumentGet { index: 3 },
                Instruction::RecordLift { type_index: 0 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(1),
                InterfaceValue::String("Hello".to_string()),
                InterfaceValue::F64(2.),
                //              ^^^ F32 is expected
                InterfaceValue::I64(3),
            ],
            instance: Instance::new(),
            error: r#"`record.lift 0` read a value of type `F64` from the stack, but the type `F32` was expected"#,
    );

    test_executable_instruction!(
        test_record_lower =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::RecordLower { type_index: 0 },
            ],
            invocation_inputs: [
                InterfaceValue::Record(vec![
                    InterfaceValue::I32(1),
                    InterfaceValue::Record(vec![
                        InterfaceValue::String("Hello".to_string()),
                        InterfaceValue::F32(2.),
                    ]),
                    InterfaceValue::I64(3),
                ])
            ],
            instance: Instance::new(),
            stack: [
                InterfaceValue::I32(1),
                InterfaceValue::String("Hello".to_string()),
                InterfaceValue::F32(2.),
                InterfaceValue::I64(3),
            ],
    );

    test_executable_instruction!(
        test_record__roundtrip =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::RecordLower { type_index: 0 },
                Instruction::RecordLift { type_index: 0 },
            ],
            invocation_inputs: [
                InterfaceValue::Record(vec![
                    InterfaceValue::I32(1),
                    InterfaceValue::Record(vec![
                        InterfaceValue::String("Hello".to_string()),
                        InterfaceValue::F32(2.),
                    ]),
                    InterfaceValue::I64(3),
                ])
            ],
            instance: Instance::new(),
            stack: [
                InterfaceValue::Record(vec![
                    InterfaceValue::I32(1),
                    InterfaceValue::Record(vec![
                        InterfaceValue::String("Hello".to_string()),
                        InterfaceValue::F32(2.),
                    ]),
                    InterfaceValue::I64(3),
                ])
            ],
    );

    test_executable_instruction!(
        test_record_lower__invalid_value_on_the_stack =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::RecordLower { type_index: 0 },
            ],
            invocation_inputs: [
                InterfaceValue::I32(1),
            ],
            instance: Instance::new(),
            error: r#"`record.lower 0` read a value of type `I32` from the stack, but the type `Record(RecordType { fields: [I32, Record(RecordType { fields: [String, F32] }), I64] })` was expected"#,
    );

    test_executable_instruction!(
        test_record_lower__invalid_value_on_the_stack__different_record_type =
            instructions: [
                Instruction::ArgumentGet { index: 0 },
                Instruction::RecordLower { type_index: 0 },
            ],
            invocation_inputs: [
                InterfaceValue::Record(vec![
                    InterfaceValue::I32(1),
                    InterfaceValue::Record(vec![
                        InterfaceValue::String("Hello".to_string()),
                    ]),
                    InterfaceValue::I64(3),
                ])
            ],
            instance: Instance::new(),
            error: r#"`record.lower 0` read a value of type `Record(RecordType { fields: [I32, Record(RecordType { fields: [String] }), I64] })` from the stack, but the type `Record(RecordType { fields: [I32, Record(RecordType { fields: [String, F32] }), I64] })` was expected"#,
    );
}
