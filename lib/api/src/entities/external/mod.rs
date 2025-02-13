pub(crate) mod extref;
pub use extref::*;

use wasmer_types::ExternType;

use crate::{
    vm::VMExtern, AsStoreMut, AsStoreRef, ExportError, Exportable, Function, Global, Memory, Table,
    Tag,
};

/// Trait convert a VMExtern to a Extern
pub trait VMExternToExtern {
    /// Convert to [`Extern`]
    fn to_extern(self, store: &mut impl AsStoreMut) -> Extern;
}

/// An `Extern` is the runtime representation of an entity that
/// can be imported or exported.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#external-values>
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub enum Extern {
    /// An external [`Function`].
    Function(Function),
    /// An external [`Global`].
    Global(Global),
    /// An external [`Table`].
    Table(Table),
    /// An external [`Memory`].
    Memory(Memory),
    /// An external [`Memory`].
    Tag(Tag),
}

impl Extern {
    /// Return the underlying type of the inner `Extern`.
    pub fn ty(&self, store: &impl AsStoreRef) -> ExternType {
        match self {
            Self::Function(ft) => ExternType::Function(ft.ty(store)),
            Self::Memory(ft) => ExternType::Memory(ft.ty(store)),
            Self::Table(tt) => ExternType::Table(tt.ty(store)),
            Self::Global(gt) => ExternType::Global(gt.ty(store)),
            Self::Tag(tt) => ExternType::Tag(tt.ty(store)),
        }
    }

    /// Create an `Extern` from an `wasmer_engine::Export`.
    pub fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExtern) -> Self {
        vm_extern.to_extern(store)
    }

    /// Checks whether this `Extern` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match self {
            Self::Function(f) => f.is_from_store(store),
            Self::Global(g) => g.is_from_store(store),
            Self::Tag(t) => t.is_from_store(store),
            Self::Memory(m) => m.is_from_store(store),
            Self::Table(t) => t.is_from_store(store),
        }
    }

    /// To `VMExtern`.
    pub fn to_vm_extern(&self) -> VMExtern {
        match self {
            Self::Function(f) => f.to_vm_extern(),
            Self::Global(g) => g.to_vm_extern(),
            Self::Tag(t) => t.to_vm_extern(),
            Self::Memory(m) => m.to_vm_extern(),
            Self::Table(t) => t.to_vm_extern(),
        }
    }
}

impl<'a> Exportable<'a> for Extern {
    fn get_self_from_extern(_extern: &'a Self) -> Result<&'a Self, ExportError> {
        // Since this is already an extern, we can just return it.
        Ok(_extern)
    }
}

impl std::fmt::Debug for Extern {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Function(_) => "Function(...)",
                Self::Global(_) => "Global(...)",
                Self::Tag(_) => "Tag(...)",
                Self::Memory(_) => "Memory(...)",
                Self::Table(_) => "Table(...)",
            }
        )
    }
}

impl From<Function> for Extern {
    fn from(r: Function) -> Self {
        Self::Function(r)
    }
}

impl From<Global> for Extern {
    fn from(r: Global) -> Self {
        Self::Global(r)
    }
}

impl From<Tag> for Extern {
    fn from(r: Tag) -> Self {
        Self::Tag(r)
    }
}

impl From<Memory> for Extern {
    fn from(r: Memory) -> Self {
        Self::Memory(r)
    }
}

impl From<Table> for Extern {
    fn from(r: Table) -> Self {
        Self::Table(r)
    }
}
