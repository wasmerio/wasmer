use crate::new;

pub use new::wasm_common::{
    //
    Bytes,
    Pages,
};

// Once https://github.com/wasmerio/wasmer-reborn/pull/47 got merged, we can replace the following lines by:
// ```rust
// pub use new::wasm_common::{WASM_MAX_PAGES, WASM_MIN_PAGES, WASM_PAGE_SIZE};
// ```
const WASM_MAX_PAGES = 0x10000;
const WASM_MIN_PAGES = 0x100;
const WASM_PAGE_SIZE = 0x10000;
