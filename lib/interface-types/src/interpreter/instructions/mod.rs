mod argument_get;
mod call_core;
mod numbers;
mod strings;

use crate::{
    errors::{InstructionError, InstructionErrorKind, InstructionResult, WasmValueNativeCastError},
    interpreter::{
        wasm::values::{InterfaceValue, NativeType},
        Instruction,
    },
};
pub(crate) use argument_get::argument_get;
pub(crate) use call_core::call_core;
pub(crate) use numbers::*;
use std::convert::TryFrom;
pub(crate) use strings::*;

/// Just a short helper to map the error of a cast from an
/// `InterfaceValue` to a native value.
pub(crate) fn to_native<'a, T>(
    wit_value: &'a InterfaceValue,
    instruction: Instruction,
) -> InstructionResult<T>
where
    T: NativeType + TryFrom<&'a InterfaceValue, Error = WasmValueNativeCastError>,
{
    T::try_from(wit_value)
        .map_err(|error| InstructionError::new(instruction, InstructionErrorKind::ToNative(error)))
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::interpreter::wasm::{
        self,
        values::{InterfaceType, InterfaceValue},
    };
    use std::{cell::Cell, collections::HashMap, convert::TryInto, ops::Deref, rc::Rc};

    pub(crate) struct Export {
        pub(crate) inputs: Vec<InterfaceType>,
        pub(crate) outputs: Vec<InterfaceType>,
        pub(crate) function: fn(arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>,
    }

    impl wasm::structures::Export for Export {
        fn inputs_cardinality(&self) -> usize {
            self.inputs.len() as usize
        }

        fn outputs_cardinality(&self) -> usize {
            self.outputs.len()
        }

        fn inputs(&self) -> &[InterfaceType] {
            &self.inputs
        }

        fn outputs(&self) -> &[InterfaceType] {
            &self.outputs
        }

        fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
            (self.function)(arguments)
        }
    }

    pub(crate) struct LocalImport {
        pub(crate) inputs: Vec<InterfaceType>,
        pub(crate) outputs: Vec<InterfaceType>,
        pub(crate) function: fn(arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>,
    }

    impl wasm::structures::LocalImport for LocalImport {
        fn inputs_cardinality(&self) -> usize {
            self.inputs.len()
        }

        fn outputs_cardinality(&self) -> usize {
            self.outputs.len()
        }

        fn inputs(&self) -> &[InterfaceType] {
            &self.inputs
        }

        fn outputs(&self) -> &[InterfaceType] {
            &self.outputs
        }

        fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
            (self.function)(arguments)
        }
    }

    #[derive(Default, Clone)]
    pub(crate) struct MemoryView(Rc<Vec<Cell<u8>>>);

    impl wasm::structures::MemoryView for MemoryView {}

    impl Deref for MemoryView {
        type Target = [Cell<u8>];

        fn deref(&self) -> &Self::Target {
            self.0.as_slice()
        }
    }

    #[derive(Default)]
    pub(crate) struct Memory {
        pub(crate) view: MemoryView,
    }

    impl Memory {
        pub(crate) fn new(data: Vec<Cell<u8>>) -> Self {
            Self {
                view: MemoryView(Rc::new(data)),
            }
        }
    }

    impl wasm::structures::Memory<MemoryView> for Memory {
        fn view(&self) -> MemoryView {
            self.view.clone()
        }
    }

    #[derive(Default)]
    pub(crate) struct Instance {
        pub(crate) exports: HashMap<String, Export>,
        pub(crate) locals_or_imports: HashMap<usize, LocalImport>,
        pub(crate) memory: Memory,
    }

    impl Instance {
        pub(crate) fn new() -> Self {
            Self {
                exports: {
                    let mut hashmap = HashMap::new();
                    hashmap.insert(
                        "sum".into(),
                        Export {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |arguments: &[InterfaceValue]| {
                                let a: i32 = (&arguments[0]).try_into().unwrap();
                                let b: i32 = (&arguments[1]).try_into().unwrap();

                                Ok(vec![InterfaceValue::I32(a + b)])
                            },
                        },
                    );

                    hashmap
                },
                locals_or_imports: {
                    let mut hashmap = HashMap::new();
                    // sum
                    hashmap.insert(
                        42,
                        LocalImport {
                            inputs: vec![InterfaceType::I32, InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |arguments: &[InterfaceValue]| {
                                let a: i32 = (&arguments[0]).try_into().unwrap();
                                let b: i32 = (&arguments[1]).try_into().unwrap();

                                Ok(vec![InterfaceValue::I32(a * b)])
                            },
                        },
                    );
                    // string allocator
                    hashmap.insert(
                        43,
                        LocalImport {
                            inputs: vec![InterfaceType::I32],
                            outputs: vec![InterfaceType::I32],
                            function: |arguments: &[InterfaceValue]| {
                                let _size: i32 = (&arguments[0]).try_into().unwrap();

                                Ok(vec![InterfaceValue::I32(0)])
                            },
                        },
                    );

                    hashmap
                },
                memory: Memory::new(vec![Cell::new(0); 128]),
            }
        }
    }

    impl wasm::structures::Instance<Export, LocalImport, Memory, MemoryView> for Instance {
        fn export(&self, export_name: &str) -> Option<&Export> {
            self.exports.get(export_name)
        }

        fn local_or_import<I: wasm::structures::TypedIndex + wasm::structures::LocalImportIndex>(
            &mut self,
            index: I,
        ) -> Option<&LocalImport> {
            self.locals_or_imports.get(&index.index())
        }

        fn memory(&self, _index: usize) -> Option<&Memory> {
            Some(&self.memory)
        }
    }
}
