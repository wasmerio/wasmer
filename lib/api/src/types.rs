use crate::externals::Function;
use crate::store::{Store, StoreObject};
use crate::RuntimeError;
use std::ptr;
use wasm_common::Value;
pub use wasm_common::{
    ExportType, ExternRef, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Type as ValType,
};

/// WebAssembly computations manipulate values of basic value types:
/// * Integers (32 or 64 bit width)
/// * Floating-point (32 or 64 bit width)
///
/// Spec: https://webassembly.github.io/spec/core/exec/runtime.html#values
pub type Val = Value<Function>;

impl StoreObject for Val {
    fn comes_from_same_store(&self, store: &Store) -> bool {
        match self {
            Val::FuncRef(f) => Store::same(store, f.store()),
            Val::ExternRef(ExternRef::Ref(_)) | Val::ExternRef(ExternRef::Other(_)) => false,
            Val::ExternRef(ExternRef::Null) => true,
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
/// from [`Val`] into `FuncRef`.
pub trait ValFuncRef {
    fn into_checked_anyfunc(
        &self,
        store: &Store,
    ) -> Result<wasmer_vm::VMCallerCheckedAnyfunc, RuntimeError>;

    fn from_checked_anyfunc(item: wasmer_vm::VMCallerCheckedAnyfunc, store: &Store) -> Self;
}

impl ValFuncRef for Val {
    fn into_checked_anyfunc(
        &self,
        store: &Store,
    ) -> Result<wasmer_vm::VMCallerCheckedAnyfunc, RuntimeError> {
        if !self.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        Ok(match self {
            Val::ExternRef(ExternRef::Null) => wasmer_vm::VMCallerCheckedAnyfunc {
                func_ptr: ptr::null(),
                type_index: wasmer_vm::VMSharedSignatureIndex::default(),
                vmctx: ptr::null_mut(),
            },
            Val::FuncRef(f) => f.checked_anyfunc(),
            _ => return Err(RuntimeError::new("val is not funcref")),
        })
    }

    fn from_checked_anyfunc(item: wasmer_vm::VMCallerCheckedAnyfunc, store: &Store) -> Val {
        if item.type_index == wasmer_vm::VMSharedSignatureIndex::default() {
            return Val::ExternRef(ExternRef::Null);
        }
        let signature = store
            .engine()
            .lookup_signature(item.type_index)
            .expect("Signature not found in store");
        let export = wasmer_vm::ExportFunction {
            address: item.func_ptr,
            signature,
            // All functions in tables are already Static (as dynamic functions
            // are converted to use the trampolines with static signatures).
            kind: wasmer_vm::VMFunctionKind::Static,
            vmctx: item.vmctx,
        };
        let f = Function::from_export(store, export);
        Val::FuncRef(f)
    }
}
