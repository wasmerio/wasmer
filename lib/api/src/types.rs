use crate::externals::Function;
use crate::store::{Store, StoreObject};
use crate::RuntimeError;
use std::ptr;
use wasmer_types::Value;
pub use wasmer_types::{
    ExportType, ExternRef, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Type as ValType,
};

/// WebAssembly computations manipulate values of basic value types:
/// * Integers (32 or 64 bit width)
/// * Floating-point (32 or 64 bit width)
/// * Vectors (128 bits, with 32 or 64 bit lanes)
///
/// Spec: https://webassembly.github.io/spec/core/exec/runtime.html#values
pub type Val = Value<Function>;

impl StoreObject for Val {
    fn comes_from_same_store(&self, store: &Store) -> bool {
        match self {
            Self::FuncRef(f) => Store::same(store, f.store()),
            Self::ExternRef(ExternRef::Ref(_)) | Self::ExternRef(ExternRef::Other(_)) => false,
            Self::ExternRef(ExternRef::Null) => true,
            Self::I32(_) | Self::I64(_) | Self::F32(_) | Self::F64(_) | Self::V128(_) => true,
        }
    }
}

impl From<Function> for Val {
    fn from(val: Function) -> Self {
        Self::FuncRef(val)
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
            Self::ExternRef(ExternRef::Null) => wasmer_vm::VMCallerCheckedAnyfunc {
                func_ptr: ptr::null(),
                type_index: wasmer_vm::VMSharedSignatureIndex::default(),
                vmctx: wasmer_vm::VMFunctionEnvironment {
                    host_env: ptr::null_mut(),
                },
            },
            Self::FuncRef(f) => f.checked_anyfunc(),
            _ => return Err(RuntimeError::new("val is not funcref")),
        })
    }

    fn from_checked_anyfunc(item: wasmer_vm::VMCallerCheckedAnyfunc, store: &Store) -> Self {
        if item.type_index == wasmer_vm::VMSharedSignatureIndex::default() {
            return Self::ExternRef(ExternRef::Null);
        }
        let signature = store
            .engine()
            .lookup_signature(item.type_index)
            .expect("Signature not found in store");
        let export = wasmer_engine::ExportFunction {
            // TODO:
            // figure out if we ever need a value here: need testing with complicated import patterns
            import_init_function_ptr: None,
            vm_function: wasmer_vm::VMExportFunction {
                address: item.func_ptr,
                signature,
                // All functions in tables are already Static (as dynamic functions
                // are converted to use the trampolines with static signatures).
                kind: wasmer_vm::VMFunctionKind::Static,
                vmctx: item.vmctx,
                call_trampoline: None,
            },
        };
        let f = Function::from_export(store, export);
        Self::FuncRef(f)
    }
}
