use js_sys::WebAssembly::Tag as JsTag;
use wasmer_types::TagType;

/// The VM Tag type
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VMTag {
    pub(crate) tag: JsTag,
    pub(crate) ty: TagType,
}

impl VMTag {
    pub(crate) fn new(tag: JsTag, ty: TagType) -> Self {
        Self { tag, ty }
    }
}

unsafe impl Send for VMTag {}
unsafe impl Sync for VMTag {}
