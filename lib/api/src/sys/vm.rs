//! The `vm` module re-exports wasmer-vm types.
use crate::externals::{Extern, Function, Global, Memory, Table, VMExternToExtern};
use crate::store::AsStoreMut;
use wasmer_vm::InternalStoreHandle;
pub(crate) use wasmer_vm::{
    VMExtern, VMExternRef, VMFuncRef, VMFunction, VMFunctionBody, VMFunctionEnvironment, VMGlobal,
    VMInstance, VMMemory, VMTable, VMTrampoline,
};

pub(crate) type VMExternTable = InternalStoreHandle<VMTable>;
pub(crate) type VMExternMemory = InternalStoreHandle<VMMemory>;
pub(crate) type VMExternGlobal = InternalStoreHandle<VMGlobal>;
pub(crate) type VMExternFunction = InternalStoreHandle<VMFunction>;

pub type VMFunctionCallback = *const VMFunctionBody;

impl VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl AsStoreMut) -> Extern {
        match self {
            Self::Function(f) => Extern::Function(Function::from_vm_extern(store, f)),
            Self::Memory(m) => Extern::Memory(Memory::from_vm_extern(store, m)),
            Self::Global(g) => Extern::Global(Global::from_vm_extern(store, g)),
            Self::Table(t) => Extern::Table(Table::from_vm_extern(store, t)),
        }
    }
}
