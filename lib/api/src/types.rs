use crate::externals::Function;
use crate::store::{Store, StoreObject};
use crate::RuntimeError;
use std::ptr;
use wasm_common::Value;
pub use wasm_common::{
    AnyRef, ExportType, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Type as ValType,
};

pub type Val = Value<Function>;

impl StoreObject for Val {
    fn comes_from_same_store(&self, store: &Store) -> bool {
        match self {
            Val::FuncRef(f) => Store::same(store, f.store()),
            Val::AnyRef(AnyRef::Ref(_)) | Val::AnyRef(AnyRef::Other(_)) => false,
            Val::AnyRef(AnyRef::Null) => true,
            Val::I32(_) | Val::I64(_) | Val::F32(_) | Val::F64(_) | Val::V128(_) => true,
        }
    }
}

impl From<Function> for Val {
    fn from(val: Function) -> Val {
        Val::FuncRef(val)
    }
}

/// It provides useful functions for converting back and forth
/// from [`Val`] into `AnyFunc`.
pub trait ValAnyFunc {
    fn into_checked_anyfunc(
        &self,
        store: &Store,
    ) -> Result<wasmer_runtime::VMCallerCheckedAnyfunc, RuntimeError>;

    fn from_checked_anyfunc(item: wasmer_runtime::VMCallerCheckedAnyfunc, store: &Store) -> Self;
}

impl ValAnyFunc for Val {
    fn into_checked_anyfunc(
        &self,
        store: &Store,
    ) -> Result<wasmer_runtime::VMCallerCheckedAnyfunc, RuntimeError> {
        if !self.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        Ok(match self {
            Val::AnyRef(AnyRef::Null) => wasmer_runtime::VMCallerCheckedAnyfunc {
                func_ptr: ptr::null(),
                type_index: wasmer_runtime::VMSharedSignatureIndex::default(),
                vmctx: ptr::null_mut(),
            },
            Val::FuncRef(f) => f.checked_anyfunc(),
            _ => return Err(RuntimeError::new("val is not funcref")),
        })
    }

    fn from_checked_anyfunc(item: wasmer_runtime::VMCallerCheckedAnyfunc, store: &Store) -> Val {
        if item.type_index == wasmer_runtime::VMSharedSignatureIndex::default() {
            return Val::AnyRef(AnyRef::Null);
        }
        let export = wasmer_runtime::ExportFunction {
            address: item.func_ptr,
            signature: item.type_index,
            vmctx: item.vmctx,
        };
        let f = Function::from_export(store, export);
        Val::FuncRef(f)
    }
}
