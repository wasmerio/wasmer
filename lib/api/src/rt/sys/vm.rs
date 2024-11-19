//! The `vm` module re-exports wasmer-vm types.
use crate::entities::{Function, Global, Memory, Table};
use crate::store::AsStoreMut;
use wasmer_vm::InternalStoreHandle;
pub(crate) use wasmer_vm::{
    Trap, TrapHandlerFn, VMConfig, VMContext, VMExtern, VMExternObj, VMExternRef, VMFuncRef,
    VMFunction, VMFunctionBody, VMFunctionEnvironment, VMGlobal, VMInstance, VMMemory,
    VMSharedMemory, VMTable, VMTrampoline,
};

pub(crate) type VMExternTable = InternalStoreHandle<VMTable>;
pub(crate) type VMExternMemory = InternalStoreHandle<VMMemory>;
pub(crate) type VMExternGlobal = InternalStoreHandle<VMGlobal>;
pub(crate) type VMExternFunction = InternalStoreHandle<VMFunction>;

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
