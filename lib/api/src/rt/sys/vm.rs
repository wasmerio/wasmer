//! The `vm` module re-exports wasmer-vm types.
use crate::entities::{Function, Global, Memory, Table};
use crate::store::AsStoreMut;
pub use wasmer_vm::*;

/// The type of extern tables in the `sys` VM.
pub type VMExternTable = InternalStoreHandle<VMTable>;
///
/// The type of extern memories in the `sys` VM.
pub type VMExternMemory = InternalStoreHandle<VMMemory>;

/// The type of extern globals in the `sys` VM.
pub type VMExternGlobal = InternalStoreHandle<VMGlobal>;

/// The type of extern functioons in the `sys` VM.
pub type VMExternFunction = InternalStoreHandle<VMFunction>;

/// The type of function callbacks in the `sys` VM.
pub type VMFunctionCallback = *const VMFunctionBody;

impl crate::VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl AsStoreMut) -> crate::Extern {
        match self {
            Self::Function(f) => crate::Extern::Function(Function::from_vm_extern(
                store,
                crate::vm::VMExternFunction::Sys(f),
            )),
            Self::Memory(m) => crate::Extern::Memory(Memory::from_vm_extern(
                store,
                crate::vm::VMExternMemory::Sys(m),
            )),
            Self::Global(g) => crate::Extern::Global(Global::from_vm_extern(
                store,
                crate::vm::VMExternGlobal::Sys(g),
            )),
            Self::Table(t) => crate::Extern::Table(Table::from_vm_extern(
                store,
                crate::vm::VMExternTable::Sys(t),
            )),
        }
    }
}
