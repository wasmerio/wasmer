use wasmer_types::TagType;

#[derive(Debug, Clone)]
pub(crate) struct WasmTagType {}

impl WasmTagType {
    pub(crate) fn new(_tag_type: TagType) -> Self {
        panic!()
    }
}
