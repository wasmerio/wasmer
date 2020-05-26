use crate::new;

pub use new::wasm_common::{
    //
    ExportType as ExportDescriptor,
    ExternType as ExternDescriptor,
    FunctionIndex as FuncIndex,
    GlobalIndex,
    GlobalInit,
    ImportType as ImportDescriptor,
    LocalFunctionIndex as LocalFuncIndex,
    LocalGlobalIndex,
    LocalMemoryIndex,
    LocalTableIndex,
    MemoryIndex,
    SignatureIndex as SigIndex,
    TableIndex,
    TableType as TableDescriptor,
    Type,
    ValueType,
};
pub use new::wasmer::Val as Value;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct GlobalDescriptor {
    pub mutable: bool,
    pub ty: Type,
}

impl From<&new::wasm_common::GlobalType> for GlobalDescriptor {
    fn from(value: &new::wasm_common::GlobalType) -> Self {
        Self {
            mutable: value.mutability.into(),
            ty: value.ty,
        }
    }
}
