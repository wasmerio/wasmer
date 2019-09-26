mod argument_get;
mod call;
mod call_export;
mod read_utf8;
mod write_utf8;

pub(crate) use argument_get::argument_get;
pub(crate) use call::call;
pub(crate) use call_export::call_export;
pub(crate) use read_utf8::read_utf8;
pub(crate) use write_utf8::write_utf8;

#[cfg(test)]
pub(crate) mod tests {
    use crate::interpreter::wasm::{
        self,
        values::{InterfaceType, InterfaceValue, ValueType},
    };
    use std::{cell::Cell, collections::HashMap, convert::TryInto};

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

    #[derive(Default)]
    pub(crate) struct Memory {
        pub(crate) data: Vec<Cell<u8>>,
    }

    impl Memory {
        pub(crate) fn new(data: Vec<Cell<u8>>) -> Self {
            Self { data }
        }
    }

    impl wasm::structures::Memory for Memory {
        fn view<V: ValueType>(&self) -> &[Cell<V>] {
            use std::slice;

            let slice = self.data.as_slice();

            unsafe { slice::from_raw_parts(slice.as_ptr() as *const Cell<V>, slice.len()) }
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
                    hashmap.insert(
                        "alloc".into(),
                        Export {
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
                locals_or_imports: {
                    let mut hashmap = HashMap::new();
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

                    hashmap
                },
                memory: Memory::new(vec![Cell::new(0); 128]),
            }
        }
    }

    impl wasm::structures::Instance<Export, LocalImport, Memory> for Instance {
        fn export(&self, export_name: &str) -> Option<&Export> {
            self.exports.get(export_name)
        }

        fn local_or_import<I: wasm::structures::TypedIndex + wasm::structures::LocalImportIndex>(
            &self,
            index: I,
        ) -> Option<&LocalImport> {
            self.locals_or_imports.get(&index.index())
        }

        fn memory(&self, _index: usize) -> Option<&Memory> {
            Some(&self.memory)
        }
    }
}
