trait WasmTypeList {
    type CStruct;
    type Array: AsMut<[i128]>;

    fn from_array(array: Self::Array) -> Self;
    fn empty_array(self) -> Self::Array;
    fn from_c_struct(c_struct: Self::CStruct) -> Self;
    fn into_c_struct(self) -> Self::CStruct;
    fn wasm_types() -> &'static [Type];
    fn from_slice(slice: &[i128]) -> Result<Self, TryFromSliceError>;
    fn into_array(self) -> Self::Array;
}
