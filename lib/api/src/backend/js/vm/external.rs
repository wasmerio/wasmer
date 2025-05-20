use wasmer_types::RawValue;

use crate::{AsStoreMut, Extern, VMExternToExtern};

use super::{function::VMFunction, global::VMGlobal, memory::VMMemory, table::VMTable, tag::VMTag};

/// The value of an export passed from one instance to another in the `js` VM.
pub enum VMExtern {
    /// A function export value.
    Function(VMFunction),

    /// A table export value.
    Table(VMTable),

    /// A memory export value.
    Memory(VMMemory),

    /// A global export value.
    Global(VMGlobal),

    /// A tag export value.
    Tag(VMTag),
}

impl VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl AsStoreMut) -> Extern {
        match self {
            Self::Function(f) => Extern::Function(crate::Function::from_vm_extern(
                store,
                crate::vm::VMExternFunction::Js(f),
            )),
            Self::Memory(m) => Extern::Memory(crate::Memory::from_vm_extern(
                store,
                crate::vm::VMExternMemory::Js(m),
            )),
            Self::Global(g) => Extern::Global(crate::Global::from_vm_extern(
                store,
                crate::vm::VMExternGlobal::Js(g),
            )),
            Self::Table(t) => Extern::Table(crate::Table::from_vm_extern(
                store,
                crate::vm::VMExternTable::Js(t),
            )),
            Self::Tag(t) => Extern::Tag(crate::Tag::from_vm_extern(
                store,
                crate::vm::VMExternTag::Js(t),
            )),
        }
    }
}

/// A reference to an external value in the `js` VM.
pub struct VMExternRef;

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
