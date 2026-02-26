use super::super::store::wasm_store_t;
use super::super::types::wasm_globaltype_t;
use super::super::value::wasm_val_t;
use super::wasm_extern_t;
use std::convert::TryInto;
use wasmer_api::{Extern, Global, Value};

#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone)]
pub struct wasm_global_t {
    pub(crate) extern_: wasm_extern_t,
}

impl wasm_global_t {
    pub(crate) fn try_from(e: &wasm_extern_t) -> Option<&wasm_global_t> {
        match &e.inner {
            Extern::Global(_) => Some(unsafe { &*(e as *const _ as *const _) }),
            _ => None,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_global_new(
    store: Option<&mut wasm_store_t>,
    global_type: Option<&wasm_globaltype_t>,
    val: Option<&wasm_val_t>,
) -> Option<Box<wasm_global_t>> {
    let global_type = global_type?;
    let store = store?;
    let mut store_mut = unsafe { store.inner.store_mut() };
    let val = val?;

    let global_type = &global_type.inner().global_type;
    let wasm_val = val.try_into().ok()?;
    let global = if global_type.mutability.is_mutable() {
        Global::new_mut(&mut store_mut, wasm_val)
    } else {
        Global::new(&mut store_mut, wasm_val)
    };
    Some(Box::new(wasm_global_t {
        extern_: wasm_extern_t::new(store.inner.clone(), global.into()),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_global_delete(_global: Option<Box<wasm_global_t>>) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_global_copy(global: &wasm_global_t) -> Box<wasm_global_t> {
    // do shallow copy
    Box::new(global.clone())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_global_get(
    global: Option<&mut wasm_global_t>,
    // own
    out: Option<&mut wasm_val_t>,
) {
    let Some(global) = global else { return };
    let Some(out) = out else { return };
    let wasm_global = global.extern_.global();
    let mut store_mut = unsafe { global.extern_.store.store_mut() };
    let value = wasm_global.get(&mut store_mut);
    if let Ok(val) = value.try_into() {
        *out = val;
    }
}

/// Note: This function returns nothing by design but it can raise an
/// error if setting a new value fails.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_global_set(
    global: Option<&mut wasm_global_t>,
    val: Option<&wasm_val_t>,
) {
    let Some(global) = global else { return };
    let Some(val) = val else { return };
    let Ok(value): Result<Value, _> = val.try_into() else {
        return;
    };
    let wasm_global = global.extern_.global();
    let mut store_mut = unsafe { global.extern_.store.store_mut() };
    c_try!(wasm_global.set(&mut store_mut, value); otherwise ());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_global_same(
    wasm_global1: &wasm_global_t,
    wasm_global2: &wasm_global_t,
) -> bool {
    wasm_global1.extern_.global() == wasm_global2.extern_.global()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_global_type(
    global: Option<&wasm_global_t>,
) -> Option<Box<wasm_globaltype_t>> {
    let global = global?;
    let store_ref = unsafe { global.extern_.store.store() };
    Some(Box::new(wasm_globaltype_t::new(
        global.extern_.global().ty(&store_ref),
    )))
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use inline_c::assert_c;
    #[cfg(target_os = "windows")]
    use wasmer_inline_c::assert_c;

    #[allow(
        unexpected_cfgs,
        reason = "tools like cargo-llvm-coverage pass --cfg coverage"
    )]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[test]
    fn test_set_host_global_immutable() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_val_t forty_two = WASM_F32_VAL(42);
                wasm_val_t forty_three = WASM_F32_VAL(43);

                wasm_valtype_t* valtype = wasm_valtype_new_i32();
                wasm_globaltype_t* global_type = wasm_globaltype_new(valtype, WASM_CONST);
                wasm_global_t* global = wasm_global_new(store, global_type, &forty_two);

                wasm_globaltype_delete(global_type);

                wasm_global_set(global, &forty_three);

                assert(wasmer_last_error_length() > 0);

                wasm_global_delete(global);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }

    #[allow(
        unexpected_cfgs,
        reason = "tools like cargo-llvm-coverage pass --cfg coverage"
    )]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[test]
    fn test_set_guest_global_immutable() {
        (assert_c! {
            #include "tests/wasmer.h"

            int main() {
                wasm_engine_t* engine = wasm_engine_new();
                wasm_store_t* store = wasm_store_new(engine);

                wasm_byte_vec_t wat;
                wasmer_byte_vec_new_from_string(&wat, "(module (global $global (export \"global\") f32 (f32.const 1)))");
                wasm_byte_vec_t wasm_bytes;
                wat2wasm(&wat, &wasm_bytes);
                wasm_module_t* module = wasm_module_new(store, &wasm_bytes);
                wasm_extern_vec_t import_object = WASM_EMPTY_VEC;
                wasm_instance_t* instance = wasm_instance_new(store, module, &import_object, NULL);

                wasm_extern_vec_t exports;
                wasm_instance_exports(instance, &exports);
                wasm_global_t* global = wasm_extern_as_global(exports.data[0]);

                wasm_val_t forty_two = WASM_F32_VAL(42);
                wasm_global_set(global, &forty_two);

                printf("%d", wasmer_last_error_length());
                assert(wasmer_last_error_length() > 0);

                wasm_instance_delete(instance);
                wasm_byte_vec_delete(&wasm_bytes);
                wasm_byte_vec_delete(&wat);
                wasm_extern_vec_delete(&exports);
                wasm_store_delete(store);
                wasm_engine_delete(engine);

                return 0;
            }
        })
        .success();
    }
}
