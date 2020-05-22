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
pub const WASM_MAX_PAGES: u32 = 0x10000;
pub const WASM_MIN_PAGES: u32 = 0x100;
pub const WASM_PAGE_SIZE: u32 = 0x10000;
