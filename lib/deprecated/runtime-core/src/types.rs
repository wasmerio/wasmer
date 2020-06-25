use crate::new;

pub use new::wasm_common::{
    //
    ExportType as ExportDescriptor,
    ExternType as ExternDescriptor,
    FunctionIndex as FuncIndex,
    FunctionIndex as ImportedFuncIndex,
    FunctionType as FuncDescriptor,
    FunctionType as FuncSig,
    GlobalIndex as ImportedGlobalIndex,
    GlobalIndex,
    GlobalInit,
    ImportType as ImportDescriptor,
    LocalFunctionIndex as LocalFuncIndex,
    LocalGlobalIndex,
    LocalMemoryIndex,
    LocalTableIndex,
    MemoryIndex as ImportedMemoryIndex,
    MemoryIndex,
    MemoryType as MemoryDescriptor,
    NativeWasmType,
    SignatureIndex as SigIndex,
    TableIndex as ImportedTableIndex,
    TableIndex,
    TableType as TableDescriptor,
    Type,
    ValueType,
};
pub use new::wasmer::{Val as Value, WasmExternType};

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
