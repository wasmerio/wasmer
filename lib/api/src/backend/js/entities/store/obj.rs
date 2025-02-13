use std::{marker::PhantomData, num::NonZeroUsize};

use wasm_bindgen::JsValue;
use wasmer_types::StoreId;

use crate::js::vm::{function::VMFunctionEnvironment, global::VMGlobal};

use super::handle::InternalStoreHandle;

/// Trait to represent an object managed by a context. This is implemented on
/// the VM types managed by the context.
pub trait StoreObject: Sized {
    fn list(store: &StoreObjects) -> &Vec<Self>;
    fn list_mut(store: &mut StoreObjects) -> &mut Vec<Self>;
}

macro_rules! impl_store_object {
    ($($field:ident => $ty:ty,)*) => {
        $(
            impl StoreObject for $ty {
                fn list(store: &StoreObjects) -> &Vec<Self> {
                    &store.$field
                }
                fn list_mut(store: &mut StoreObjects) -> &mut Vec<Self> {
                    &mut store.$field
                }
            }
        )*
    };
}

impl_store_object! {
    // Note: we store the globals in order to be able to access them later via
    // `StoreObjects::iter_globals`.
    globals => VMGlobal,
    // functions => VMFunction,
    // tables => VMTable,
    // memories => VMMemory,
    // The function environments are the only things attached to a store,
    // since the other JS objects (table, globals, memory and functions)
    // live in the JS VM Store by default
    function_environments => VMFunctionEnvironment,
}

/// Set of objects managed by a context.
#[derive(Default, Debug)]
pub struct StoreObjects {
    id: StoreId,
    globals: Vec<VMGlobal>,
    function_environments: Vec<VMFunctionEnvironment>,
}

impl StoreObjects {
    /// Returns the ID of this context.
    pub fn id(&self) -> StoreId {
        self.id
    }

    /// Sets the ID of this store
    pub fn set_id(&mut self, id: StoreId) {
        self.id = id;
    }

    /// Returns a pair of mutable references from two handles.
    ///
    /// Panics if both handles point to the same object.
    pub fn get_2_mut<T: StoreObject>(
        &mut self,
        a: InternalStoreHandle<T>,
        b: InternalStoreHandle<T>,
    ) -> (&mut T, &mut T) {
        assert_ne!(a.index(), b.index());
        let list = T::list_mut(self);
        if a.index() < b.index() {
            let (low, high) = list.split_at_mut(b.index());
            (&mut low[a.index()], &mut high[0])
        } else {
            let (low, high) = list.split_at_mut(a.index());
            (&mut high[0], &mut low[a.index()])
        }
    }

    /// Return an immutable iterator over all globals
    pub fn iter_globals(&self) -> core::slice::Iter<VMGlobal> {
        self.globals.iter()
    }

    /// Return an vector of all globals and converted to u128
    pub fn as_u128_globals(&self) -> Vec<u128> {
        self.iter_globals()
            .map(|v| v.global.value().as_f64().unwrap() as u128)
            .collect()
    }

    /// Set a global, at index idx. Will panic if idx is out of range
    /// Safety: the caller should check that the raw value is compatible
    /// with destination VMGlobal type
    pub fn set_global_unchecked(&self, idx: usize, new_val: u128) {
        assert!(idx < self.globals.len());

        let g = &self.globals[idx].global;
        let cur_val = g.value().as_f64().unwrap();
        let new_val = new_val as f64;
        if cur_val != new_val {
            let new_value = JsValue::from(new_val);
            g.set_value(&new_value);
        }
    }
}

impl crate::StoreObjects {
    /// Consume [`self`] into [`crate::backend::js::store::StoreObjects`].
    pub fn into_js(self) -> crate::backend::js::store::StoreObjects {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::store::StoreObjects`].
    pub fn as_js(&self) -> &crate::backend::js::store::StoreObjects {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::store::StoreObjects`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::store::StoreObjects {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }
}
