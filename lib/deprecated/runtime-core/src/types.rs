use crate::new;

pub use new::wasm_common::{
    //
    ExportType as ExportDescriptor,
    ExternType as ExternDescriptor,
    FunctionIndex as FuncIndex,
    GlobalIndex,
    GlobalInit,
    GlobalType as GlobalDescriptor,
    ImportType as ImportDescriptor,
    LocalFunctionIndex as LocalFuncIndex,
    LocalGlobalIndex,
    LocalMemoryIndex,
    LocalTableIndex,
    MemoryIndex,
    SignatureIndex as SigIndex,
    TableIndex,
};
pub use new::wasmer::{Val as Value, ValType as Type};
