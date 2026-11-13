use wasmer_types::StoreId;

use crate::{BackendStore, macros::backend::match_rt};

/// Set of objects managed by a context.
#[derive(Debug)]
pub enum StoreObjects {
    #[cfg(feature = "sys")]
    /// Store objects for the `sys` runtime.
    Sys(crate::backend::sys::store::StoreObjects),

    #[cfg(feature = "v8")]
    /// Store objects for the `v8` runtime.
    V8(crate::backend::v8::store::StoreObjects),

    #[cfg(feature = "js")]
    /// Store objects for the `js` runtime.
    Js(crate::backend::js::store::StoreObjects),
}

impl StoreObjects {
    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    #[inline]
    pub fn same(a: &Self, b: &Self) -> bool {
        match (a, b) {
            #[cfg(feature = "sys")]
            (Self::Sys(a), Self::Sys(b)) => a.id() == b.id(),
            #[cfg(feature = "v8")]
            (Self::V8(a), Self::V8(b)) => a.id() == b.id(),
            #[cfg(feature = "js")]
            (Self::Js(a), Self::Js(b)) => a.id() == b.id(),

            _ => panic!(
                "Incompatible `StoreObjects` instance: {}, {}!",
                a.id(),
                b.id()
            ),
        }
    }

    /// Returns the ID of this store
    #[inline]
    pub fn id(&self) -> StoreId {
        match_rt!(on self => s {
            s.id()
        })
    }

    #[inline]
    pub(crate) fn from_store_ref(store: &BackendStore) -> Self {
        match store {
            #[cfg(feature = "sys")]
            BackendStore::Sys(_) => Self::Sys(Default::default()),
            #[cfg(feature = "v8")]
            BackendStore::V8(_) => Self::V8(Default::default()),
            #[cfg(feature = "js")]
            BackendStore::Js(_) => Self::Js(Default::default()),
        }
    }

    /// Return a vector of all globals and converted to u128
    #[inline]
    pub fn as_u128_globals(&self) -> Vec<u128> {
        match_rt!(on self => s {
            s.as_u128_globals()
        })
    }

    /// Set a global, at index idx. Will panic if idx is out of range
    /// Safety: the caller should check that the raw value is compatible
    /// with destination VMGlobal type
    #[inline]
    pub fn set_global_unchecked(&self, idx: usize, val: u128) {
        match_rt!(on self => s {
            s.set_global_unchecked(idx, val)
        })
    }

    /// Builds an [`Interrupter`](crate::Interrupter) for this store.
    #[cfg(all(
        unix,
        feature = "sys",
        feature = "experimental-host-interrupt",
        not(feature = "baremetal")
    ))]
    pub fn interrupter(&self) -> crate::Interrupter {
        match self {
            Self::Sys(s) => crate::Interrupter::new(s.id()),
            _ => panic!("Interrupters can only be built for stores from the sys backend"),
        }
    }
}
