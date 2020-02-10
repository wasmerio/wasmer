//! A stack-based interpreter to execute instructions of WIT adapters.

mod instruction;
mod instructions;
pub mod stack;
pub mod wasm;

pub use instruction::Instruction;
use stack::Stack;
use std::{convert::TryFrom, marker::PhantomData};
use wasm::values::InterfaceValue;

pub(crate) struct Runtime<'invocation, 'instance, Instance, Export, LocalImport, Memory, MemoryView>
where
    Export: wasm::structures::Export + 'instance,
    LocalImport: wasm::structures::LocalImport + 'instance,
    Memory: wasm::structures::Memory<MemoryView> + 'instance,
    MemoryView: wasm::structures::MemoryView,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory, MemoryView> + 'instance,
{
    invocation_inputs: &'invocation [InterfaceValue],
    stack: Stack<InterfaceValue>,
    wasm_instance: &'instance mut Instance,
    _phantom: PhantomData<(Export, LocalImport, Memory, MemoryView)>,
}

pub(crate) type ExecutableInstruction<Instance, Export, LocalImport, Memory, MemoryView> = Box<
    dyn Fn(&mut Runtime<Instance, Export, LocalImport, Memory, MemoryView>) -> Result<(), String>,
>;

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

    pub fn run(
        &self,
        invocation_inputs: &[InterfaceValue],
        wasm_instance: &mut Instance,
    ) -> Result<Stack<InterfaceValue>, String> {
        let mut runtime = Runtime {
            invocation_inputs,
            stack: Stack::new(),
            wasm_instance,
            _phantom: PhantomData,
        };

        for executable_instruction in self.iter() {
            match executable_instruction(&mut runtime) {
                Ok(_) => continue,
                Err(message) => return Err(message),
            }
        }

        Ok(runtime.stack)
    }
}

impl<'binary_input, Instance, Export, LocalImport, Memory, MemoryView>
    TryFrom<&Vec<Instruction<'binary_input>>>
    for Interpreter<Instance, Export, LocalImport, Memory, MemoryView>
where
    Export: wasm::structures::Export,
    LocalImport: wasm::structures::LocalImport,
    Memory: wasm::structures::Memory<MemoryView>,
    MemoryView: wasm::structures::MemoryView,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory, MemoryView>,
{
    type Error = String;

    fn try_from(instructions: &Vec<Instruction>) -> Result<Self, Self::Error> {
        let executable_instructions = instructions
            .iter()
            .map(|instruction| {
                let instruction_name: String = instruction.into();

                match instruction {
                    Instruction::ArgumentGet { index } => {
                        instructions::argument_get(*index, instruction_name)
                    }
                    Instruction::Call { function_index } => {
                        instructions::call(*function_index, instruction_name)
                    }
                    Instruction::CallExport { export_name } => {
                        instructions::call_export((*export_name).to_owned(), instruction_name)
                    }
                    Instruction::ReadUtf8 => instructions::read_utf8(instruction_name),
                    Instruction::WriteUtf8 { allocator_name } => {
                        instructions::write_utf8((*allocator_name).to_owned(), instruction_name)
                    }
                    _ => unimplemented!(),
                }
            })
            .collect();

        Ok(Interpreter {
            executable_instructions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{wasm::structures::EmptyMemoryView, Instruction, Interpreter};
    use std::convert::TryInto;

    #[test]
    fn test_interpreter_from_instructions() {
        let instructions = vec![
            Instruction::ArgumentGet { index: 0 },
            Instruction::ArgumentGet { index: 0 },
            Instruction::CallExport { export_name: "foo" },
            Instruction::ReadUtf8,
            Instruction::Call { function_index: 7 },
        ];
        let interpreter: Interpreter<(), (), (), (), EmptyMemoryView> =
            (&instructions).try_into().unwrap();

        assert_eq!(interpreter.executable_instructions.len(), 5);
    }
}
