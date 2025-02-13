//! Data types, functions and traits for `v8` runtime's `Tag` implementation.
use wasmer_types::{TagType, Type};

use crate::{
    v8::{
        bindings::*,
        utils::convert::{IntoCApiType, IntoWasmerType},
        vm::VMTag,
    },
    vm::{VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef,
};

use super::check_isolate;

#[derive(Debug, Clone, PartialEq, Eq)]
/// A WebAssembly `tag` in the `v8` runtime.
pub(crate) struct Tag {
    pub(crate) handle: VMTag,
}

unsafe impl Send for Tag {}
unsafe impl Sync for Tag {}

impl Tag {
    pub fn new<P: Into<Box<[Type]>>>(store: &mut impl AsStoreMut, params: P) -> Self {
        check_isolate(store);
        let store_mut = store.as_store_mut();
        let v8_store = store_mut.inner.store.as_v8();

        let params = Into::<Box<[Type]>>::into(params).into_vec();
        let params = params
            .into_iter()
            .map(|param| {
                let kind = param.into_ct();
                unsafe { wasm_valtype_new(kind) }
            })
            .collect::<Vec<_>>();

        let mut params = unsafe {
            let mut vec: wasm_valtype_vec_t = Default::default();
            wasm_valtype_vec_new(&mut vec, params.len(), params.as_ptr());
            vec
        };

        let tag_type = unsafe { wasm_tagtype_new(&mut params) };
        if tag_type.is_null() {
            panic!("failed to create new tag: returned tag type is null");
        }

        let handle = unsafe { wasm_tag_new(v8_store.inner, tag_type) };
        if handle.is_null() {
            panic!("failed to create new tag: returned tag is null");
        }

        Self { handle }
    }

    pub fn ty(&self, store: &impl AsStoreRef) -> TagType {
        check_isolate(store);
        let type_ = unsafe { wasm_tag_type(self.handle) };
        let params: *const wasm_valtype_vec_t = unsafe { wasm_tagtype_params(type_) };

        let params: Vec<wasmer_types::Type> = unsafe {
            let mut res = vec![];
            for i in 0..(*params).size {
                res.push((*(*params).data.wrapping_add(i)).into_wt());
            }
            res
        };

        TagType::new(wasmer_types::TagKind::Exception, params, vec![])
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_tag: VMExternTag) -> Self {
        check_isolate(store);
        Self {
            handle: vm_tag.into_v8(),
        }
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        check_isolate(store);
        true
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        let extern_ = unsafe { wasm_tag_as_extern(self.handle) };
        assert!(
            !extern_.is_null(),
            "Returned null Tag extern from wasm-c-api"
        );

        VMExtern::V8(extern_)
    }
}
