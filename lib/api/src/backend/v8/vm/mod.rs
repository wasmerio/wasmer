mod env;
pub use env::*;

use super::{
    bindings::*, entities::function::env::FunctionEnv, function::Function, global::Global,
    memory::Memory, table::Table,
};
use crate::{AsStoreMut, BackendFunction, BackendGlobal, BackendMemory, BackendTable, Extern};
use wasmer_types::RawValue;

pub use super::error::Trap;

pub(crate) type VMExtern = *mut wasm_extern_t;

// No EH for now.
pub(crate) type VMException = ();
pub(crate) type VMTag = *mut wasm_tag_t;
pub(crate) type VMExternTag = *mut wasm_tag_t;

pub(crate) type VMFunction = *mut wasm_func_t;
pub(crate) type VMFunctionBody = ();
pub(crate) type VMFunctionCallback = *mut ::std::os::raw::c_void;
pub(crate) type VMTrampoline = *mut ::std::os::raw::c_void;
pub(crate) type VMExternFunction = *mut wasm_func_t;

pub(crate) type VMGlobal = *mut wasm_global_t;
pub(crate) type VMExternGlobal = *mut wasm_global_t;

pub(crate) type VMMemory = *mut wasm_memory_t;
pub type VMSharedMemory = VMMemory;
pub(crate) type VMExternMemory = *mut wasm_memory_t;

pub(crate) type VMTable = *mut wasm_table_t;
pub(crate) type VMExternTable = *mut wasm_table_t;

pub(crate) type VMInstance = *mut wasm_instance_t;

pub(crate) type VMExternObj = ();
pub(crate) type VMConfig = ();

impl crate::VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl AsStoreMut) -> Extern {
        let kind = unsafe { wasm_extern_kind(&mut *self) };

        match kind as u32 {
            0 => {
                let func = unsafe { wasm_extern_as_func(&mut *self) };
                if func.is_null() {
                    panic!("V8 reported extern as function, but is not");
                }
                Extern::Function(crate::Function::from_vm_extern(
                    store,
                    crate::vm::VMExternFunction::V8(func),
                ))
            }
            1 => {
                let global = unsafe { wasm_extern_as_global(&mut *self) };
                if global.is_null() {
                    panic!("V8 reported extern as a global, but is not");
                }
                Extern::Global(crate::Global::from_vm_extern(
                    store,
                    crate::vm::VMExternGlobal::V8(global),
                ))
            }
            2 => {
                let table = unsafe { wasm_extern_as_table(&mut *self) };
                if table.is_null() {
                    panic!("V8 reported extern as a table, but is not");
                }
                Extern::Table(crate::Table::from_vm_extern(
                    store,
                    crate::vm::VMExternTable::V8(table),
                ))
            }
            3 => {
                let memory = unsafe { wasm_extern_as_memory(&mut *self) };
                if memory.is_null() {
                    panic!("V8 reported extern as a memory, but is not");
                }
                Extern::Memory(crate::Memory::from_vm_extern(
                    store,
                    crate::vm::VMExternMemory::V8(memory),
                ))
            }
            4 => {
                let tag = unsafe { wasm_extern_as_tag(&mut *self) };
                if tag.is_null() {
                    panic!("V8 reported extern as a tag, but is not");
                }
                Extern::Tag(crate::Tag::from_vm_extern(
                    store,
                    crate::vm::VMExternTag::V8(tag),
                ))
            }
            _ => {
                unimplemented!()
            }
        }
    }
}

pub(crate) struct VMExternRef(*mut wasm_ref_t);
impl VMExternRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        unimplemented!()
    }

    /// Extracts a `VMExternRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExternRef` instance.
    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

pub(crate) struct VMFuncRef(*mut wasm_ref_t);
impl VMFuncRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        unimplemented!()
    }

    /// Extracts a `VMExternRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExternRef` instance.
    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

pub(crate) struct VMExceptionRef(*mut wasm_ref_t);
impl VMExceptionRef {
    /// Converts the `VMExceptionRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        unimplemented!()
    }

    /// Extracts a `VMExceptionRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExceptionRef` instance.
    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}
