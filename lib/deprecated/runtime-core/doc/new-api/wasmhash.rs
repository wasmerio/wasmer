struct WasmHash {}

impl WasmHash {
    fn generate(wasm: &[u8]) -> Self;
    fn encode(self) -> String;
    fn decode(hex_str: &str) -> Result<Self, DeserializeError>;
}
