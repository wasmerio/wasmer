use js_sys::wasm_bindgen;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// [MDN documentation](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/Memory)
    #[wasm_bindgen(js_namespace = WebAssembly, extends = js_sys::Object, typescript_type = "WebAssembly.Memory")]
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub type JSMemory;

    /// The `grow()` protoype method of the `Memory` object increases the
    /// size of the memory instance by a specified number of WebAssembly
    /// pages.
    ///
    /// Takes the number of pages to grow (64KiB in size) and returns the
    /// previous size of memory, in pages.
    ///
    /// # Reimplementation
    ///
    /// We re-implement `WebAssembly.Memory.grow` because it is
    /// different from what `wasm-bindgen` declares. It marks the function
    /// as `catch`, which means it can throw an exception.
    ///
    /// See [the opened patch](https://github.com/rustwasm/wasm-bindgen/pull/2599).
    ///
    /// # Exceptions
    ///
    /// A `RangeError` is thrown if adding pages would exceed the maximum
    /// memory.
    ///
    /// [MDN documentation](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/Memory/grow)
    #[wasm_bindgen(catch, method, js_namespace = WebAssembly)]
    pub fn grow(this: &JSMemory, pages: u32) -> Result<u32, JsValue>;
}
