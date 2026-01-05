use js_sys::wasm_bindgen;
use wasm_bindgen::prelude::*;
use std::sync::OnceLock;


#[wasm_bindgen]
extern "C" {

    #[wasm_bindgen(js_namespace = WebAssembly, js_name = Suspending)]
    pub fn suspending(func: &JsValue) -> JsValue;

    #[wasm_bindgen(js_name = promising, js_namespace = WebAssembly)]
    pub fn promising(func: &JsValue) -> JsValue;
}


#[wasm_bindgen(inline_js = r#"
export function has_jspi() {
  const WA = globalThis.WebAssembly;
  return !!(WA && WA.promising && WA.Suspending);
}
"#)]
extern "C" {
    fn has_jspi() -> bool;
}

static JSPI_SUPPORTED: OnceLock<bool> = OnceLock::new();

pub fn supports_jspi() -> bool {
    *JSPI_SUPPORTED.get_or_init(|| has_jspi())
}
