#![cfg(all(target_arch = "wasm32", feature = "browser"))]

use wasm_bindgen_test::wasm_bindgen_test;
use wasmer::{Memory, MemoryType, Store};

#[wasm_bindgen_test]
fn idle_realm_retirement_waits_for_live_engine_owners() {
    assert!(wasixv2::browser::can_retire());
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(2), true)).unwrap();
    let shared = memory.as_shared(&store).unwrap();
    assert!(!wasixv2::browser::can_retire());
    drop(memory);
    drop(store);
    assert!(!wasixv2::browser::can_retire());
    drop(shared);
    assert!(wasixv2::browser::can_retire());
}
