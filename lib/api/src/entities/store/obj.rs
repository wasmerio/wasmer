use wasmer_types::StoreId;

use crate::RuntimeStore;

/// Set of objects managed by a context.
#[derive(Debug)]
pub enum StoreObjects {
    #[cfg(feature = "sys")]
    /// Store objects for the `sys` runtime.
    Sys(crate::rt::sys::store::StoreObjects),

    #[cfg(feature = "wamr")]
    /// Store objects for the `wamr` runtime.
    Wamr(crate::rt::wamr::store::StoreObjects),

    #[cfg(feature = "v8")]
    /// Store objects for the `v8` runtime.
    V8(crate::rt::v8::store::StoreObjects),
}

impl StoreObjects {
    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    pub fn same(a: &Self, b: &Self) -> bool {
        match (a, b) {
            #[cfg(feature = "sys")]
            (Self::Sys(ref a), Self::Sys(ref b)) => a.id() == b.id(),
            #[cfg(feature = "wamr")]
            (Self::Wamr(ref a), Self::Wamr(ref b)) => a.id() == b.id(),

            #[cfg(feature = "v8")]
            (Self::V8(ref a), Self::V8(ref b)) => a.id() == b.id(),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Returns the ID of this store
    pub fn id(&self) -> StoreId {
        match &self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.id(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.id(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.id(),
            _ => panic!("No runtime enabled!"),
        }
    }

    pub(crate) fn from_store_ref(store: &RuntimeStore) -> Self {
        match store {
            #[cfg(feature = "sys")]
            RuntimeStore::Sys(_) => Self::Sys(Default::default()),
            #[cfg(feature = "wamr")]
            RuntimeStore::Wamr(_) => Self::Wamr(Default::default()),

            #[cfg(feature = "v8")]
            RuntimeStore::V8(_) => Self::V8(Default::default()),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Return a vector of all globals and converted to u128
    pub fn as_u128_globals(&self) -> Vec<u128> {
        match &self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.as_u128_globals(),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.as_u128_globals(),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.as_u128_globals(),
            _ => panic!("No runtime enabled!"),
        }
    }

    /// Set a global, at index idx. Will panic if idx is out of range
    /// Safety: the caller should check taht the raw value is compatible
    /// with destination VMGlobal type
    pub fn set_global_unchecked(&self, idx: usize, val: u128) {
        match &self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.set_global_unchecked(idx, val),
            #[cfg(feature = "wamr")]
            Self::Wamr(s) => s.set_global_unchecked(idx, val),
            #[cfg(feature = "v8")]
            Self::V8(s) => s.set_global_unchecked(idx, val),
            _ => panic!("No runtime enabled!"),
        }
    }
}
