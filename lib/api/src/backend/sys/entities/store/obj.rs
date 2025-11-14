use wasmer_vm::StoreId;

use crate::AsStoreMut;

impl crate::StoreObjects {
    /// Consume store objects into [`crate::backend::sys::store::StoreObjects`].
    pub fn into_sys(self) -> crate::backend::sys::store::StoreObjects {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }

    /// Convert a reference to store objects into a reference [`crate::backend::sys::store::StoreObjects`].
    pub fn as_sys(&self) -> &crate::backend::sys::store::StoreObjects {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }

    /// Convert a mutable reference to store objects into a mutable reference [`crate::backend::sys::store::StoreObjects`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::store::StoreObjects {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }
}
