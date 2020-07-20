trait WasmExternType {
    type Native: NativeWasmType;

    fn from_native(native: Self::Native) -> Self;
    fn to_native(self) -> Self::Native;
}
