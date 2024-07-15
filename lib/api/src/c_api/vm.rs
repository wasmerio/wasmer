use super::bindings::{
    wasm_extern_t, wasm_func_t, wasm_global_t, wasm_instance_t, wasm_memory_t, wasm_ref_t,
    wasm_table_t,
};
use std::any::Any;
/// This module is mainly used to create the `VM` types that will hold both
/// the JS values of the `Memory`, `Table`, `Global` and `Function` and also
/// it's types.
/// This module should not be needed any longer (with the exception of the memory)
/// once the type reflection is added to the WebAssembly JS API.
/// https://github.com/WebAssembly/js-types/
// use crate::store::AsStoreRef;
use wasmer_types::RawValue;
// use std::any::Any;
// use std::fmt;
// use tracing::trace;
// use wasmer_types::RawValue;
// use wasmer_types::{
//     FunctionType, GlobalType, MemoryError, MemoryType, Pages, TableType, WASM_PAGE_SIZE,
// };

// pub(crate) type VMFunctionEnvironment = *mut ::std::os::raw::c_void;

/// The VM Function type
pub type VMFunction = *mut wasm_func_t;
/// The VM Memory type
pub type VMMemory = *mut wasm_memory_t;
/// The VM Shared Memory type
pub type VMSharedMemory = VMMemory;
/// The VM Global type
pub type VMGlobal = *mut wasm_global_t;
/// The VM Table type
pub type VMTable = *mut wasm_table_t;
// pub(crate) type VMExternRef = *mut wasm_ref_t;
pub(crate) type VMExtern = *mut wasm_extern_t;
// pub(crate) type VMFuncRef = *mut wasm_ref_t;
pub(crate) type VMInstance = *mut wasm_instance_t;

pub(crate) type VMExternTable = VMTable;
pub(crate) type VMExternMemory = VMMemory;
pub(crate) type VMExternGlobal = VMGlobal;
pub(crate) type VMExternFunction = VMFunction;

pub type VMFunctionCallback = *mut ::std::os::raw::c_void;
// pub type VMTrampoline = *mut ::std::os::raw::c_void;

use crate::bindings::{
    wasm_extern_as_func, wasm_extern_as_global, wasm_extern_as_memory, wasm_extern_as_table,
    wasm_extern_kind, wasm_extern_type, wasm_externkind_enum_WASM_EXTERN_FUNC,
    wasm_externkind_enum_WASM_EXTERN_GLOBAL, wasm_externkind_enum_WASM_EXTERN_MEMORY,
};
use crate::externals::{Extern, Function, Global, Memory, Table, VMExternToExtern};
use crate::store::AsStoreMut;

impl VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl AsStoreMut) -> Extern {
        let kind = unsafe { wasm_extern_kind(&mut *self) };

        match kind as u32 {
            0 => {
                let func = unsafe { wasm_extern_as_func(&mut *self) };
                if func.is_null() {
                    panic!("The wasm-c-api reported extern as function, but is not");
                }
                Extern::Function(Function::from_vm_extern(store, func))
            }
            1 => {
                let global = unsafe { wasm_extern_as_global(&mut *self) };
                if global.is_null() {
                    panic!("The wasm-c-api reported extern as a global, but is not");
                }
                Extern::Global(Global::from_vm_extern(store, global))
            }
            2 => {
                let table = unsafe { wasm_extern_as_table(&mut *self) };
                if table.is_null() {
                    panic!("The wasm-c-api reported extern as a table, but is not");
                }
                Extern::Table(Table::from_vm_extern(store, table))
            }
            3 => {
                let memory = unsafe { wasm_extern_as_memory(&mut *self) };
                if memory.is_null() {
                    panic!("The wasm-c-api reported extern as a table, but is not");
                }
                Extern::Memory(Memory::from_vm_extern(store, memory))
            }
            _ => {
                unimplemented!()
            }
        }
        // match self {
        //     Self::Function(f) => Extern::Function(Function::from_vm_extern(store, f)),
        //     Self::Memory(m) => Extern::Memory(Memory::from_vm_extern(store, m)),
        //     Self::Global(g) => Extern::Global(Global::from_vm_extern(store, g)),
        //     Self::Table(t) => Extern::Table(Table::from_vm_extern(store, t)),
        // }
    }
}

/// Underlying FunctionEnvironment used by a `VMFunction`.
#[derive(Debug)]
pub struct VMFunctionEnvironment {
    contents: Box<dyn Any + Send + 'static>,
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            contents: Box::new(val),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        &*self.contents
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        &mut *self.contents
    }
}

pub(crate) struct VMExternRef;

impl VMExternRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        unimplemented!();
    }

    /// Extracts a `VMExternRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExternRef` instance.
    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VMFuncRef;

impl VMFuncRef {
    /// Converts the `VMFuncRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        unimplemented!();
    }

    /// Extracts a `VMFuncRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw.funcref` must be a valid pointer.
    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

pub struct VMTrampoline;
