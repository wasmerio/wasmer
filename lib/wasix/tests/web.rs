//! Integration tests that exercise the `js` feature in a browser.
//!
//! Note that we can't run tests in a NodeJS environment because the threadpool
//! implementation uses `wasm_bindgen::module()`, which is a hidden browser-only
//! API.
#![cfg(all(target_arch = "wasm32", feature = "js"))]

use futures::channel::oneshot;
use wasmer_wasix::{
    http::default_http_client,
    runtime::{
        resolver::WapmSource::WASMER_PROD_ENDPOINT,
        task_manager::{
            web::{WebTaskManager, WebThreadPool},
            VirtualTaskManager,
        },
    },
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test::wasm_bindgen_test]
async fn use_the_task_manager() {
    let pool = WebThreadPool::new(1).unwrap();
    let task_manager = WebTaskManager::new(pool);
    let (sender, receiver) = oneshot::channel();

    task_manager
        .task_shared(Box::new(move || {
            Box::pin(async move {
                sender.send(42_u32).unwrap();
            })
        }))
        .unwrap();

    assert_eq!(receiver.await.unwrap(), 42);
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn send_a_post_request() {
    let http_client = wasmer_wasix::http::web::WebHttpClient::default();
    let query = r#"{
        "query": "{ info { defaultFrontend } }"
    }"#;
    let request = http::Request::post(WASMER_PROD_ENDPOINT)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(query);

    let response = http_client.request(request.into()).await.unwrap();

    panic!("{response:?}");
}
