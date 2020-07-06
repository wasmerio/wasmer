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
pub use new::wasmer::{FromToNativeWasmType as WasmExternType, Val as Value};

/// Describes the mutability and type of a Global
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_types_round_trip() {
        assert_eq!(
            42i32,
            i32::from_native(i32::from_binary((42i32).to_native().to_binary()))
        );

        assert_eq!(
            -42i32,
            i32::from_native(i32::from_binary((-42i32).to_native().to_binary()))
        );

        use std::i64;
        let xi64 = i64::MAX;
        assert_eq!(
            xi64,
            i64::from_native(i64::from_binary((xi64).to_native().to_binary()))
        );
        let yi64 = i64::MIN;
        assert_eq!(
            yi64,
            i64::from_native(i64::from_binary((yi64).to_native().to_binary()))
        );

        assert_eq!(
            16.5f32,
            f32::from_native(f32::from_binary((16.5f32).to_native().to_binary()))
        );

        assert_eq!(
            -16.5f32,
            f32::from_native(f32::from_binary((-16.5f32).to_native().to_binary()))
        );

        use std::f64;
        let xf64: f64 = f64::MAX;
        assert_eq!(
            xf64,
            f64::from_native(f64::from_binary((xf64).to_native().to_binary()))
        );

        let yf64: f64 = f64::MIN;
        assert_eq!(
            yf64,
            f64::from_native(f64::from_binary((yf64).to_native().to_binary()))
        );
    }
}
