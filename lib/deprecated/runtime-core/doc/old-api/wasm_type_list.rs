trait WasmTypeList {
    type CStruct;
    type RetArray: AsMut<[u64]>;

    fn from_ret_array(array: Self::RetArray) -> Self;
    fn empty_ret_array() -> Self::RetArray;
    fn from_c_struct(c_struct: Self::CStruct) -> Self;
    fn into_c_struct(self) -> Self::CStruct;
    fn types() -> &'static [Type];
    fn call<Rets>(self, f: NonNull<Func>, wasm: Wasm, ctx: *mut Ctx) -> Result<Rets, RuntimeError>;
}
