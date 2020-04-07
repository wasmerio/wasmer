//! A stack-based interpreter to execute instructions of WIT adapters.

mod instructions;
pub mod stack;
pub mod wasm;

use crate::errors::{InstructionResult, InterpreterResult};
pub use instructions::Instruction;
use stack::Stack;
use std::{convert::TryFrom, marker::PhantomData};
use wasm::values::InterfaceValue;

/// Represents the `Runtime`, which is used by an adapter to execute
/// its instructions.
pub(crate) struct Runtime<'invocation, 'instance, Instance, Export, LocalImport, Memory, MemoryView>
where
    Export: wasm::structures::Export + 'instance,
    LocalImport: wasm::structures::LocalImport + 'instance,
    Memory: wasm::structures::Memory<MemoryView> + 'instance,
    MemoryView: wasm::structures::MemoryView,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory, MemoryView> + 'instance,
{
    /// The invocation inputs are all the arguments received by an
    /// adapter.
    invocation_inputs: &'invocation [InterfaceValue],

    /// Each runtime (so adapter) has its own stack instance.
    stack: Stack<InterfaceValue>,

    /// The WebAssembly module instance. It is used by adapter's
    /// instructions.
    wasm_instance: &'instance mut Instance,

    /// Phantom data.
    _phantom: PhantomData<(Export, LocalImport, Memory, MemoryView)>,
}

/// Type alias for an executable instruction. It's an implementation
/// details, but an instruction is a boxed closure instance.
pub(crate) type ExecutableInstruction<Instance, Export, LocalImport, Memory, MemoryView> = Box<
    dyn Fn(
        &mut Runtime<Instance, Export, LocalImport, Memory, MemoryView>,
    ) -> InstructionResult<()>,
>;

/// An interpreter is the central piece of this crate. It is a set of
/// executable instructions. Each instruction takes the runtime as
/// argument. The runtime holds the invocation inputs, [the
/// stack](stack), and [the WebAssembly instance](wasm).
///
/// When the interpreter executes the instructions, each of them can
/// query the WebAssembly instance, operates on the stack, or reads
/// the invocation inputs. At the end of the execution, the stack
/// supposedly contains a result. Since an interpreter is used by a
/// WIT adapter to execute its instructions, the result on the stack
/// is the result of the adapter.
///
/// # Example
///
/// ```rust,ignore
/// use std::{cell::Cell, collections::HashMap, convert::TryInto};
/// use wasmer_interface_types::interpreter::{
///     instructions::tests::{Export, Instance, LocalImport, Memory, MemoryView},
/// //  ^^^^^^^^^^^^ This is private and for testing purposes only.
/// //               It is basically a fake WebAssembly runtime.
///     stack::Stackable,
///     wasm::values::{InterfaceType, InterfaceValue},
///     Instruction, Interpreter,
/// };
///
/// // 1. Creates an interpreter from a set of instructions. They will
/// //    be transformed into executable instructions.
/// let interpreter: Interpreter<Instance, Export, LocalImport, Memory, MemoryView> = (&vec![
///     Instruction::ArgumentGet { index: 1 },
///     Instruction::ArgumentGet { index: 0 },
///     Instruction::CallCore { function_index: 42 },
/// ])
///     .try_into()
///     .unwrap();
///
/// // 2. Defines the arguments of the adapter.
/// let invocation_inputs = vec![InterfaceValue::I32(3), InterfaceValue::I32(4)];
///
/// // 3. Creates a WebAssembly instance.
/// let mut instance = Instance {
///     // 3.1. Defines one function: `fn sum(a: i32, b: i32) -> i32 { a + b }`.
///     locals_or_imports: {
///         let mut hashmap = HashMap::new();
///         hashmap.insert(
///             42,
///             LocalImport {
///                 // Defines the argument types of the function.
///                 inputs: vec![InterfaceType::I32, InterfaceType::I32],
///
///                 // Defines the result types.
///                 outputs: vec![InterfaceType::I32],
///
///                 // Defines the function implementation.
///                 function: |arguments: &[InterfaceValue]| {
///                     let a: i32 = (&arguments[0]).try_into().unwrap();
///                     let b: i32 = (&arguments[1]).try_into().unwrap();
///
///                     Ok(vec![InterfaceValue::I32(a + b)])
///                 },
///             },
///         );
///     },
///     ..Default::default()
/// };
///
/// // 4. Executes the instructions.
/// let run = interpreter.run(&invocation_inputs, &mut instance);
///
/// assert!(run.is_ok());
///
/// let stack = run.unwrap();
///
/// // 5. Read the stack to get the result.
/// assert_eq!(stack.as_slice(), &[InterfaceValue::I32(7)]);
/// ```
pub struct Interpreter<Instance, Export, LocalImport, Memory, MemoryView>
where
    Export: wasm::structures::Export,
    LocalImport: wasm::structures::LocalImport,
    Memory: wasm::structures::Memory<MemoryView>,
    MemoryView: wasm::structures::MemoryView,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory, MemoryView>,
{
    executable_instructions:
        Vec<ExecutableInstruction<Instance, Export, LocalImport, Memory, MemoryView>>,
}

impl<Instance, Export, LocalImport, Memory, MemoryView>
    Interpreter<Instance, Export, LocalImport, Memory, MemoryView>
where
    Export: wasm::structures::Export,
    LocalImport: wasm::structures::LocalImport,
    Memory: wasm::structures::Memory<MemoryView>,
    MemoryView: wasm::structures::MemoryView,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory, MemoryView>,
{
    fn iter(
        &self,
    ) -> impl Iterator<
        Item = &ExecutableInstruction<Instance, Export, LocalImport, Memory, MemoryView>,
    > + '_ {
        self.executable_instructions.iter()
    }

    /// Runs the interpreter, such as:
    ///   1. Create a fresh stack,
    ///   2. Create a fresh stack,
    ///   3. Execute the instructions one after the other, and
    ///      returns the stack.
    pub fn run(
        &self,
        invocation_inputs: &[InterfaceValue],
        wasm_instance: &mut Instance,
    ) -> InterpreterResult<Stack<InterfaceValue>> {
        let mut runtime = Runtime {
            invocation_inputs,
            stack: Stack::new(),
            wasm_instance,
            _phantom: PhantomData,
        };

        for executable_instruction in self.iter() {
            executable_instruction(&mut runtime)?;
        }

        Ok(runtime.stack)
    }
}

/// Transforms a `Vec<Instruction>` into an `Interpreter`.
impl<Instance, Export, LocalImport, Memory, MemoryView> TryFrom<&Vec<Instruction>>
    for Interpreter<Instance, Export, LocalImport, Memory, MemoryView>
where
    Export: wasm::structures::Export,
    LocalImport: wasm::structures::LocalImport,
    Memory: wasm::structures::Memory<MemoryView>,
    MemoryView: wasm::structures::MemoryView,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory, MemoryView>,
{
    type Error = ();

    fn try_from(instructions: &Vec<Instruction>) -> Result<Self, Self::Error> {
        let executable_instructions = instructions
            .iter()
            .map(|instruction| match instruction {
                Instruction::ArgumentGet { index } => {
                    instructions::argument_get(*index, *instruction)
                }

                Instruction::CallCore { function_index } => {
                    instructions::call_core(*function_index, *instruction)
                }

                Instruction::S8FromI32 => instructions::s8_from_i32(*instruction),
                Instruction::S8FromI64 => instructions::s8_from_i64(*instruction),
                Instruction::S16FromI32 => instructions::s16_from_i32(*instruction),
                Instruction::S16FromI64 => instructions::s16_from_i64(*instruction),
                Instruction::S32FromI32 => instructions::s32_from_i32(*instruction),
                Instruction::S32FromI64 => instructions::s32_from_i64(*instruction),
                Instruction::S64FromI32 => instructions::s64_from_i32(*instruction),
                Instruction::S64FromI64 => instructions::s64_from_i64(*instruction),
                Instruction::I32FromS8 => instructions::i32_from_s8(*instruction),
                Instruction::I32FromS16 => instructions::i32_from_s16(*instruction),
                Instruction::I32FromS32 => instructions::i32_from_s32(*instruction),
                Instruction::I32FromS64 => instructions::i32_from_s64(*instruction),
                Instruction::I64FromS8 => instructions::i64_from_s8(*instruction),
                Instruction::I64FromS16 => instructions::i64_from_s16(*instruction),
                Instruction::I64FromS32 => instructions::i64_from_s32(*instruction),
                Instruction::I64FromS64 => instructions::i64_from_s64(*instruction),
                Instruction::U8FromI32 => instructions::u8_from_i32(*instruction),
                Instruction::U8FromI64 => instructions::u8_from_i64(*instruction),
                Instruction::U16FromI32 => instructions::u16_from_i32(*instruction),
                Instruction::U16FromI64 => instructions::u16_from_i64(*instruction),
                Instruction::U32FromI32 => instructions::u32_from_i32(*instruction),
                Instruction::U32FromI64 => instructions::u32_from_i64(*instruction),
                Instruction::U64FromI32 => instructions::u64_from_i32(*instruction),
                Instruction::U64FromI64 => instructions::u64_from_i64(*instruction),
                Instruction::I32FromU8 => instructions::i32_from_u8(*instruction),
                Instruction::I32FromU16 => instructions::i32_from_u16(*instruction),
                Instruction::I32FromU32 => instructions::i32_from_u32(*instruction),
                Instruction::I32FromU64 => instructions::i32_from_u64(*instruction),
                Instruction::I64FromU8 => instructions::i64_from_u8(*instruction),
                Instruction::I64FromU16 => instructions::i64_from_u16(*instruction),
                Instruction::I64FromU32 => instructions::i64_from_u32(*instruction),
                Instruction::I64FromU64 => instructions::i64_from_u64(*instruction),

                Instruction::StringLiftMemory => instructions::string_lift_memory(*instruction),
                Instruction::StringLowerMemory { allocator_index } => {
                    instructions::string_lower_memory(*allocator_index, *instruction)
                }
                Instruction::StringSize => instructions::string_size(*instruction),

                Instruction::RecordLift { type_index } => {
                    instructions::record_lift(*type_index, *instruction)
                }
                Instruction::RecordLower { type_index } => {
                    instructions::record_lower(*type_index, *instruction)
                }
            })
            .collect();

        Ok(Interpreter {
            executable_instructions,
        })
    }
}
