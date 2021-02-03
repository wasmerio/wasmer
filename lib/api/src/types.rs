use crate::externals::Function;
use crate::store::{Store, StoreObject};
use crate::RuntimeError;
use wasmer_types::Value;
pub use wasmer_types::{
    ExportType, ExternRef, ExternType, FunctionType, GlobalType, HostInfo, HostRef, ImportType,
    MemoryType, Mutability, TableType, Type as ValType,
};
use wasmer_vm::VMFuncRef;

/// WebAssembly computations manipulate values of basic value types:
/// * Integers (32 or 64 bit width)
/// * Floating-point (32 or 64 bit width)
/// * Vectors (128 bits, with 32 or 64 bit lanes)
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#values>
pub type Val = Value<Function>;

impl StoreObject for Val {
    fn comes_from_same_store(&self, store: &Store) -> bool {
        match self {
            Self::FuncRef(None) => true,
            Self::FuncRef(Some(f)) => Store::same(store, f.store()),
            Self::ExternRef(ExternRef::Ref(_)) | Self::ExternRef(ExternRef::Other(_)) => false,
            Self::ExternRef(ExternRef::Null) => todo!("update this code"),
            Self::I32(_) | Self::I64(_) | Self::F32(_) | Self::F64(_) | Self::V128(_) => true,
        }
    }
}

impl From<Function> for Val {
    fn from(val: Function) -> Self {
        Self::FuncRef(Some(val))
    }
}

/// It provides useful functions for converting back and forth
/// from [`Val`] into `FuncRef`.
pub trait ValFuncRef {
    fn into_checked_anyfunc(&self, store: &Store) -> Result<VMFuncRef, RuntimeError>;

    fn from_checked_anyfunc(item: VMFuncRef, store: &Store) -> Self;

    fn into_table_reference(
        &self,
        store: &Store,
    ) -> Result<wasmer_vm::TableReference, RuntimeError>;

    fn from_table_reference(item: wasmer_vm::TableReference, store: &Store) -> Self;
}

impl ValFuncRef for Val {
    fn into_checked_anyfunc(&self, store: &Store) -> Result<VMFuncRef, RuntimeError> {
        if !self.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        Ok(match self {
            Self::ExternRef(ExternRef::Null) => todo!("Extern ref not yet implemented"), /*wasmer_vm::VMCallerCheckedAnyfunc {
            func_ptr: ptr::null(),
            type_index: wasmer_vm::VMSharedSignatureIndex::default(),
            vmctx: wasmer_vm::VMFunctionEnvironment {
            host_env: ptr::null_mut(),
            },
            },*/
            Self::FuncRef(None) => VMFuncRef::null(),
            Self::FuncRef(Some(f)) => f.checked_anyfunc(),
            _ => return Err(RuntimeError::new("val is not reference")),
        })
    }

    fn from_checked_anyfunc(func_ref: VMFuncRef, store: &Store) -> Self {
        if func_ref.is_null() {
            return Self::FuncRef(None);
        }
        let item: &wasmer_vm::VMCallerCheckedAnyfunc = unsafe { &**func_ref };
        let signature = store
            .engine()
            .lookup_signature(item.type_index)
            .expect("Signature not found in store");
        let export = wasmer_engine::ExportFunction {
            // TODO:
            // figure out if we ever need a value here: need testing with complicated import patterns
            metadata: None,
            vm_function: wasmer_vm::VMExportFunction {
                address: item.func_ptr,
                signature,
                // All functions in tables are already Static (as dynamic functions
                // are converted to use the trampolines with static signatures).
                kind: wasmer_vm::VMFunctionKind::Static,
                vmctx: item.vmctx,
                call_trampoline: None,
                instance_ref: None,
            },
        };
        let f = Function::from_vm_export(store, export);
        Self::FuncRef(Some(f))
    }

    fn into_table_reference(
        &self,
        store: &Store,
    ) -> Result<wasmer_vm::TableReference, RuntimeError> {
        if !self.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        Ok(match self {
            Self::ExternRef(ExternRef::Null) =>
            /*wasmer_vm::TableReference::FuncRef(wasmer_vm::VMCallerCheckedAnyfunc {
                func_ptr: ptr::null(),
                type_index: wasmer_vm::VMSharedSignatureIndex::default(),
                vmctx: wasmer_vm::VMFunctionEnvironment {
                    host_env: ptr::null_mut(),
                },
            }),*/
            // existing code uses `ExtenRef` for null pointers
            {
                todo!("extern ref not yet supported")
            }
            Self::FuncRef(None) => wasmer_vm::TableReference::FuncRef(VMFuncRef::null()),
            Self::FuncRef(Some(f)) => wasmer_vm::TableReference::FuncRef(f.checked_anyfunc()),
            _ => return Err(RuntimeError::new("val is not reference")),
        })
    }

    fn from_table_reference(item: wasmer_vm::TableReference, store: &Store) -> Self {
        match item {
            wasmer_vm::TableReference::FuncRef(f) => Self::from_checked_anyfunc(f, store),
            wasmer_vm::TableReference::ExternRef(_f) => todo!("extern ref not yet implemented"), //Self::ExternRef(f.from_checked_anyfunc(store)),
        }
    }
}
