mod instruction;
mod instructions;
pub mod stack;
pub mod wasm;

pub use instruction::Instruction;
use stack::Stack;
use std::{convert::TryFrom, marker::PhantomData};
use wasm::values::InterfaceValue;

pub(crate) struct Runtime<'invocation, 'instance, Instance, Export, LocalImport, Memory>
where
    Export: wasm::structures::Export + 'instance,
    LocalImport: wasm::structures::LocalImport + 'instance,
    Memory: wasm::structures::Memory + 'instance,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory> + 'instance,
{
    invocation_inputs: &'invocation [InterfaceValue],
    stack: Stack<InterfaceValue>,
    wasm_instance: &'instance Instance,
    _wasm_exports: PhantomData<Export>,
    _wasm_locals_or_imports: PhantomData<LocalImport>,
    _wasm_memories: PhantomData<Memory>,
}

pub(crate) type ExecutableInstruction<Instance, Export, LocalImport, Memory> =
    Box<dyn Fn(&mut Runtime<Instance, Export, LocalImport, Memory>) -> Result<(), String>>;

pub struct Interpreter<Instance, Export, LocalImport, Memory>
where
    Export: wasm::structures::Export,
    LocalImport: wasm::structures::LocalImport,
    Memory: wasm::structures::Memory,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory>,
{
    executable_instructions: Vec<ExecutableInstruction<Instance, Export, LocalImport, Memory>>,
}

impl<Instance, Export, LocalImport, Memory> Interpreter<Instance, Export, LocalImport, Memory>
where
    Export: wasm::structures::Export,
    LocalImport: wasm::structures::LocalImport,
    Memory: wasm::structures::Memory,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory>,
{
    fn iter(
        &self,
    ) -> impl Iterator<Item = &ExecutableInstruction<Instance, Export, LocalImport, Memory>> + '_
    {
        self.executable_instructions.iter()
    }

    pub fn run(
        &self,
        invocation_inputs: &[InterfaceValue],
        wasm_instance: &Instance,
    ) -> Result<Stack<InterfaceValue>, String> {
        let mut runtime = Runtime {
            invocation_inputs,
            stack: Stack::new(),
            wasm_instance,
            _wasm_exports: PhantomData,
            _wasm_locals_or_imports: PhantomData,
            _wasm_memories: PhantomData,
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

impl<'binary_input, Instance, Export, LocalImport, Memory> TryFrom<&Vec<Instruction<'binary_input>>>
    for Interpreter<Instance, Export, LocalImport, Memory>
where
    Export: wasm::structures::Export,
    LocalImport: wasm::structures::LocalImport,
    Memory: wasm::structures::Memory,
    Instance: wasm::structures::Instance<Export, LocalImport, Memory>,
{
    type Error = String;

    fn try_from(instructions: &Vec<Instruction>) -> Result<Self, Self::Error> {
        let executable_instructions = instructions
            .iter()
            .map(
                |instruction| -> ExecutableInstruction<Instance, Export, LocalImport, Memory> {
                    let instruction_representation: String = instruction.into();

                    match instruction {
                        Instruction::ArgumentGet { index } => {
                            instructions::argument_get(*index, instruction_representation)
                        }
                        Instruction::Call { function_index } => {
                            instructions::call(*function_index, instruction_representation)
                        }
                        Instruction::CallExport { export_name } => instructions::call_export(
                            (*export_name).to_owned(),
                            instruction_representation,
                        ),
                        Instruction::ReadUtf8 => {
                            instructions::read_utf8(instruction_representation)
                        }
                        Instruction::WriteUtf8 { allocator_name } => instructions::write_utf8(
                            (*allocator_name).to_owned(),
                            instruction_representation,
                        ),
                        _ => unimplemented!(),
                    }
                },
            )
            .collect();

        Ok(Interpreter {
            executable_instructions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Instruction, Interpreter};
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
        let interpreter: Interpreter<(), (), (), ()> = (&instructions).try_into().unwrap();

        assert_eq!(interpreter.executable_instructions.len(), 5);
    }
}
