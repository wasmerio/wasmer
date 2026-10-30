#![cfg(all(feature = "js", target_arch = "wasm32"))]

use wasm_bindgen::prelude::*;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use wasmer::{Memory, MemoryType, Store, js::AsJs};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen(
    inline_js = "export function contend(memory) { const url = URL.createObjectURL(new Blob([`onmessage = ({data: memory}) => { const words = new Int32Array(memory.buffer); for (let i = 0; i < 50000; i++) { let old = Atomics.load(words, 0); while (Atomics.compareExchange(words, 0, old, old + 1) !== old) old = Atomics.load(words, 0); } postMessage('done'); };`], {type:'text/javascript'})); const workers = [new Worker(url), new Worker(url)]; URL.revokeObjectURL(url); return Promise.all(workers.map(worker => new Promise((resolve, reject) => { worker.onerror = e => { worker.terminate(); reject(Error(e.message)); }; worker.onmessage = () => { worker.terminate(); resolve(); }; worker.postMessage(memory); }))); }"
)]
extern "C" {
    fn contend(memory: JsValue) -> js_sys::Promise;
}

#[wasm_bindgen_test]
async fn compare_exchange_with_two_guest_workers_loses_no_updates() {
    let mut store = Store::default();
    let memory = Memory::new(&mut store, MemoryType::new(1, Some(2), true)).unwrap();
    let workers = contend(memory.as_jsvalue(&store));
    let view = memory.as_js().view(&store);
    for _ in 0..50000 {
        let mut old = view.compare_exchange_u32(0, 0, 0).unwrap();
        loop {
            let actual = view.compare_exchange_u32(0, old, old + 1).unwrap();
            if actual == old {
                break;
            }
            old = actual;
        }
    }
    wasm_bindgen_futures::JsFuture::from(workers).await.unwrap();
    assert_eq!(view.compare_exchange_u32(0, 0, 0).unwrap(), 150000);
}
