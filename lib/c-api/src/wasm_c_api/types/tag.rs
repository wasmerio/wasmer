use wasmer_types::TagType;

use super::wasm_valtype_vec_t;

#[allow(unused)]
pub(crate) struct WasmTagType {
    pub(crate) tag_type: TagType,
    params: wasm_valtype_vec_t,
}

impl WasmTagType {
    pub(crate) fn new(tag_type: TagType) -> Self {
        Self {
            params: tag_type
                .params()
                .iter()
                .map(|&valtype| Some(Box::new(valtype.into())))
                .collect::<Vec<_>>()
                .into(),
            tag_type,
        }
    }
}

impl Clone for WasmTagType {
    fn clone(&self) -> Self {
        Self::new(self.tag_type.clone())
    }
}

impl std::fmt::Debug for WasmTagType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.tag_type.fmt(f)
    }
}
