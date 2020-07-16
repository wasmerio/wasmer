trait NativeWasmType {
    type Abi: Copy + std::fmt::Debug;
    const WASM_TYPE: Type;

    fn from_binary(binary: i128) -> Self;
    fn to_binary(self) -> i128;
    fn into_abi(self) -> Self::Abi;
    fn from_abi(abi: Self::Abi) -> Self;
    fn to_value<T>(self) -> Value<T>;
}
