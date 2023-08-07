//! Integration tests that exercise the `js` feature in a browser.
//!
//! Note that we can't run tests in a NodeJS environment because the threadpool
//! implementation uses `wasm_bindgen::module()`, which is a hidden browser-only
//! API.
#![cfg(all(target_arch = "wasm32", feature = "js"))]

use futures::channel::oneshot;
use wasmer_wasix::{
    http::HttpClient,
    runtime::{
        resolver::WapmSource,
        task_manager::{VirtualTaskManager, WebTaskManager, WebThreadPool},
    },
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn init_logging() {
    static ONCE: std::sync::Once = std::sync::Once::new();

    ONCE.call_once(|| {
        let _ = tracing_wasm::set_as_global_default();
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    });
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn atomics_are_enabled() {
    assert!(cfg!(target_feature = "atomics"));
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn use_the_task_manager() {
    init_logging();
    tracing::info!(
        is_wasm = cfg!(target_arch = "wasm32"),
        atomics_enabled = cfg!(target_feature = "atomics"),
    );
    let pool = WebThreadPool::new(2);
    let task_manager = WebTaskManager::new(pool);
    let (sender, receiver) = oneshot::channel();

    task_manager
        .task_shared(Box::new(move || {
            Box::pin(async move {
                sender.send(42_u32).unwrap();
            })
        }))
        .unwrap();

    tracing::info!("Waiting for result");
    let result = receiver.await.unwrap();
    tracing::info!("Received {result}");
    assert_eq!(result, 42);
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn query_the_wasmer_registry_graphql_endpoint() {
    init_logging();
    let http_client = wasmer_wasix::http::web_http_client::WebHttpClient::default();
    let query = r#"{
        "query": "{ info { defaultFrontend } }"
    }"#;
    let request = http::Request::post(WapmSource::WASMER_PROD_ENDPOINT)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(query)
        .unwrap();

    let response = http_client.request(request.into()).await.unwrap();

    assert_eq!(
        response
            .headers
            .get(http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json",
    );
    let body: serde_json::Value =
        serde_json::from_slice(response.body.as_deref().unwrap()).unwrap();
    let frontend_url = body
        .pointer("/data/info/defaultFrontend")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(frontend_url, "https://wasmer.io");
}
