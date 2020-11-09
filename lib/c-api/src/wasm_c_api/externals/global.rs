use super::super::store::wasm_store_t;
use super::super::types::wasm_globaltype_t;
use super::super::value::wasm_val_t;
use std::convert::TryInto;
use wasmer::{Global, Val};

#[allow(non_camel_case_types)]
pub struct wasm_global_t {
    // maybe needs to hold onto instance
    pub(crate) inner: Global,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_new(
    store: &wasm_store_t,
    global_type: &wasm_globaltype_t,
    val: &wasm_val_t,
) -> Option<Box<wasm_global_t>> {
    let global_type = &global_type.inner().global_type;
    let wasm_val = val.try_into().ok()?;
    let store = &store.inner;
    let global = if global_type.mutability.is_mutable() {
        Global::new_mut(store, wasm_val)
    } else {
        Global::new(store, wasm_val)
    };

    Some(Box::new(wasm_global_t { inner: global }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_delete(_global: Option<Box<wasm_global_t>>) {}

// TODO: figure out if these should be deep or shallow copies
#[no_mangle]
pub unsafe extern "C" fn wasm_global_copy(wasm_global: &wasm_global_t) -> Box<wasm_global_t> {
    // do shallow copy
    Box::new(wasm_global_t {
        inner: wasm_global.inner.clone(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_get(wasm_global: &wasm_global_t, out: &mut wasm_val_t) {
    let value = wasm_global.inner.get();
    *out = value.try_into().unwrap();
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_set(wasm_global: &mut wasm_global_t, val: &wasm_val_t) {
    let value: Val = val.try_into().unwrap();
    wasm_global.inner.set(value);
}

#[no_mangle]
pub unsafe extern "C" fn wasm_global_same(
    wasm_global1: &wasm_global_t,
    wasm_global2: &wasm_global_t,
) -> bool {
    wasm_global1.inner.same(&wasm_global2.inner)
}

#[no_mangle]
pub extern "C" fn wasm_global_type(wasm_global: &wasm_global_t) -> Box<wasm_globaltype_t> {
    Box::new(wasm_globaltype_t::new(wasm_global.inner.ty().clone()))
}
