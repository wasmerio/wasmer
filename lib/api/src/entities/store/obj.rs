use wasmer_types::StoreId;

use crate::{macros::backend::match_rt, BackendStore};

/// Set of objects managed by a context.
#[derive(Debug)]
pub enum StoreObjects {
    #[cfg(feature = "sys")]
    /// Store objects for the `sys` runtime.
    Sys(crate::backend::sys::store::StoreObjects),

    #[cfg(feature = "wamr")]
    /// Store objects for the `wamr` runtime.
    Wamr(crate::backend::wamr::store::StoreObjects),

    #[cfg(feature = "wasmi")]
    /// Store objects for the `wasmi` runtime.
    Wasmi(crate::backend::wasmi::store::StoreObjects),

    #[cfg(feature = "v8")]
    /// Store objects for the `v8` runtime.
    V8(crate::backend::v8::store::StoreObjects),

    #[cfg(feature = "js")]
    /// Store objects for the `js` runtime.
    Js(crate::backend::js::store::StoreObjects),

    #[cfg(feature = "jsc")]
    /// Store objects for the `jsc` runtime.
    Jsc(crate::backend::jsc::store::StoreObjects),
}

impl StoreObjects {
    /// Checks whether two stores are identical. A store is considered
    /// equal to another store if both have the same engine.
    #[inline]
    pub fn same(a: &Self, b: &Self) -> bool {
        match (a, b) {
            #[cfg(feature = "sys")]
            (Self::Sys(ref a), Self::Sys(ref b)) => a.id() == b.id(),
            #[cfg(feature = "wamr")]
            (Self::Wamr(ref a), Self::Wamr(ref b)) => a.id() == b.id(),
            #[cfg(feature = "v8")]
            (Self::V8(ref a), Self::V8(ref b)) => a.id() == b.id(),
            #[cfg(feature = "js")]
            (Self::Js(ref a), Self::Js(ref b)) => a.id() == b.id(),

            #[cfg(feature = "jsc")]
            (Self::Jsc(ref a), Self::Jsc(ref b)) => a.id() == b.id(),

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
            #[cfg(feature = "wamr")]
            BackendStore::Wamr(_) => Self::Wamr(Default::default()),
            #[cfg(feature = "wasmi")]
            BackendStore::Wasmi(_) => Self::Wasmi(Default::default()),
            #[cfg(feature = "v8")]
            BackendStore::V8(_) => Self::V8(Default::default()),
            #[cfg(feature = "js")]
            BackendStore::Js(_) => Self::Js(Default::default()),
            #[cfg(feature = "jsc")]
            BackendStore::Jsc(_) => Self::Jsc(Default::default()),
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
    /// Safety: the caller should check taht the raw value is compatible
    /// with destination VMGlobal type
    #[inline]
    pub fn set_global_unchecked(&self, idx: usize, val: u128) {
        match_rt!(on self => s {
            s.set_global_unchecked(idx, val)
        })
    }
}
