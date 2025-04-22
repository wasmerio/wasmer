use wasmer_types::{TagType, Type};

use crate::{
    js::vm::VMTag,
    vm::{VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef, BackendTag, TagKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `tag` in the `v8` runtime.
pub(crate) struct Tag {
    pub(crate) handle: VMTag,
}

unsafe impl Send for Tag {}
unsafe impl Sync for Tag {}

// Tag can't be Send in js because it dosen't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Tag {}

impl Tag {
    pub fn new<P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, params: P) -> Self {
        let descriptor = js_sys::Object::new();
        let params: Box<[Type]> = params.into();
        let parameters: Vec<String> = params
            .into_iter()
            .map(|param| match param {
                Type::I32 => "i32".to_string(),
                Type::I64 => "i64".to_string(),
                Type::F32 => "f32".to_string(),
                Type::F64 => "f64".to_string(),
                _ => unimplemented!("The type is not yet supported in the JS Global API"),
            })
            .collect();
        js_sys::Reflect::set(&descriptor, &"parameters".into(), &parameters.into()).unwrap();

        let tag = js_sys::WebAssembly::Tag::new(&descriptor);
        let ty = TagType::new(TagKind::Exception, params);
        let handle = VMTag::new(tag.unwrap(), ty);
        Self { handle }
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> TagType {
        self.handle.ty.clone()
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternTag) -> Self {
        Self {
            handle: vm_extern.into_js(),
        }
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        true
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Js(crate::js::vm::VMExtern::Tag(self.handle.clone()))
    }
}

impl crate::Tag {
    /// Consume [`self`] into [`crate::backend::js::tag::Tag`].
    pub fn into_js(self) -> crate::backend::js::tag::Tag {
        match self.0 {
            BackendTag::Js(s) => s,
            _ => panic!("Not a `js` tag!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::tag::Tag`].
    pub fn as_js(&self) -> &crate::backend::js::tag::Tag {
        match self.0 {
            BackendTag::Js(ref s) => s,
            _ => panic!("Not a `js` tag!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::tag::Tag`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::tag::Tag {
        match self.0 {
            BackendTag::Js(ref mut s) => s,
            _ => panic!("Not a `js` tag!"),
        }
    }
}
