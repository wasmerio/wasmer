trait NativeWasmType {
    const TYPE: Type;

    fn from_binary(bits: u64) -> Self;
    fn to_binary(self) -> u64;
}
