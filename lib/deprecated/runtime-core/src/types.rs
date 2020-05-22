use crate::new;

pub use new::wasmer::{Val as Value, ValType as Type};

pub use new::wasm_common::{
    //
    FunctionIndex as FuncIndex,
    GlobalIndex,
    GlobalInit,
    LocalFunctionIndex as LocalFuncIndex,
    LocalGlobalIndex,
    LocalMemoryIndex,
    LocalTableIndex,
    MemoryIndex,
    SignatureIndex as SigIndex,
    TableIndex,
};
