#![cfg(all(feature = "js", target_arch = "wasm32"))]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{Function, FunctionEnv, FunctionType, Store, js::AsJs};

struct Captured(Arc<AtomicUsize>);
impl Captured {
    fn touch(&self) {
        assert_eq!(self.0.load(Ordering::Relaxed), 0);
    }
}
impl Drop for Captured {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[wasm_bindgen_test]
fn store_drop_releases_sync_host_captures_even_if_js_keeps_the_function() {
    for results in 0..=2 {
        let drops = Arc::new(AtomicUsize::new(0));
        let captured = Captured(drops.clone());
        let mut store = Store::default();
        let env = FunctionEnv::new(&mut store, ());
        let function = Function::new_with_env(
            &mut store,
            &env,
            FunctionType::new([], vec![wasmer::Type::I32; results]),
            move |_, _| {
                captured.touch();
                Ok(vec![wasmer::Value::I32(7); results])
            },
        );
        let js: js_sys::Function = function.as_jsvalue(&store).unchecked_into();
        js.call0(&JsValue::UNDEFINED).unwrap();
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        drop(store);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        // A stale JS caller must get a JS error rather than access a freed Store.
        assert!(js.call0(&JsValue::UNDEFINED).is_err());
        drop(function);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
}

#[cfg(feature = "experimental-async")]
#[wasm_bindgen_test]
fn store_drop_releases_async_host_captures_without_a_gc_turn() {
    let drops = Arc::new(AtomicUsize::new(0));
    let captured = Captured(drops.clone());
    let mut store = Store::default();
    let function = Function::new_async(&mut store, FunctionType::new([], []), move |_| {
        captured.touch();
        async { Ok(vec![]) }
    });
    let js: js_sys::Function = function.as_jsvalue(&store).unchecked_into();
    drop(store);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert!(js.call0(&JsValue::UNDEFINED).is_err());
    drop(function);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}
