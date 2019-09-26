use super::values::{InterfaceType, InterfaceValue, ValueType};
use std::cell::Cell;

pub trait TypedIndex: Copy + Clone {
    fn new(index: usize) -> Self;
    fn index(&self) -> usize;
}

macro_rules! typed_index {
    ($type:ident) => {
        #[derive(Copy, Clone)]
        pub struct $type(usize);

        impl TypedIndex for $type {
            fn new(index: usize) -> Self {
                Self(index)
            }

            fn index(&self) -> usize {
                self.0
            }
        }
    };
}

typed_index!(FunctionIndex);
typed_index!(LocalFunctionIndex);
typed_index!(ImportFunctionIndex);

pub trait LocalImportIndex {
    type Local: TypedIndex;
    type Import: TypedIndex;
}

impl LocalImportIndex for FunctionIndex {
    type Local = LocalFunctionIndex;
    type Import = ImportFunctionIndex;
}

pub trait Export {
    fn inputs_cardinality(&self) -> usize;
    fn outputs_cardinality(&self) -> usize;
    fn inputs(&self) -> &[InterfaceType];
    fn outputs(&self) -> &[InterfaceType];
    fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>;
}

pub trait LocalImport {
    fn inputs_cardinality(&self) -> usize;
    fn outputs_cardinality(&self) -> usize;
    fn inputs(&self) -> &[InterfaceType];
    fn outputs(&self) -> &[InterfaceType];
    fn call(&self, arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()>;
}

pub trait Memory {
    fn view<V: ValueType>(&self) -> &[Cell<V>];
}

pub trait Instance<E, LI, M>
where
    E: Export,
    LI: LocalImport,
    M: Memory,
{
    fn export(&self, export_name: &str) -> Option<&E>;
    fn local_or_import<I: TypedIndex + LocalImportIndex>(&self, index: I) -> Option<&LI>;
    fn memory(&self, index: usize) -> Option<&M>;
}

impl Export for () {
    fn inputs_cardinality(&self) -> usize {
        0
    }

    fn outputs_cardinality(&self) -> usize {
        0
    }

    fn inputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn outputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn call(&self, _arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
        Err(())
    }
}

impl LocalImport for () {
    fn inputs_cardinality(&self) -> usize {
        0
    }

    fn outputs_cardinality(&self) -> usize {
        0
    }

    fn inputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn outputs(&self) -> &[InterfaceType] {
        &[]
    }

    fn call(&self, _arguments: &[InterfaceValue]) -> Result<Vec<InterfaceValue>, ()> {
        Err(())
    }
}

impl Memory for () {
    fn view<V: ValueType>(&self) -> &[Cell<V>] {
        &[]
    }
}

impl<E, LI, M> Instance<E, LI, M> for ()
where
    E: Export,
    LI: LocalImport,
    M: Memory,
{
    fn export(&self, _export_name: &str) -> Option<&E> {
        None
    }

    fn memory(&self, _: usize) -> Option<&M> {
        None
    }

    fn local_or_import<I: TypedIndex + LocalImportIndex>(&self, _index: I) -> Option<&LI> {
        None
    }
}
