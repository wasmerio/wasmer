use std::{marker::PhantomData, num::NonZeroUsize};

use wasm_bindgen::JsValue;
use wasmer_types::StoreId;

use crate::js::vm::{function::VMFunctionEnvironment, global::VMGlobal};

pub use wasmer_types::{StoreHandle, InternalStoreHandle};

wasmer_types::impl_object_store!(StoreObjects<Object> {
    // Note: we store the globals in order to be able to access them later via
    // `StoreObjects::iter_globals`.
    globals: VMGlobal,
    // functions: VMFunction,
    // tables: VMTable,
    // memories: VMMemory,
    // The function environments are the only things attached to a store,
    // since the other JS objects (table, globals, memory and functions)
    // live in the JS VM Store by default
    function_environments: VMFunctionEnvironment<Object>,
});

/// Set of objects managed by a context.
#[derive_where::derive_where(Default, Debug)]
pub struct StoreObjects<Object = wasmer_types::BoxStoreObject> {
    id: StoreId,
    globals: Vec<VMGlobal>,
    function_environments: Vec<VMFunctionEnvironment<Object>>,
}

impl<Object> StoreObjects<Object> {
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

impl<Object> crate::StoreObjects<Object> {
    /// Consume [`self`] into [`crate::backend::js::store::StoreObjects`].
    pub fn into_js(self) -> crate::backend::js::store::StoreObjects<Object> {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::store::StoreObjects`].
    pub fn as_js(&self) -> &crate::backend::js::store::StoreObjects<Object> {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::store::StoreObjects`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::store::StoreObjects<Object> {
        match self {
            Self::Js(s) => s,
            _ => panic!("Not a `js` store!"),
        }
    }
}
