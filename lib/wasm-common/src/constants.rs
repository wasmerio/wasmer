/// WebAssembly page sizes are fixed to be 64KiB.
/// Note: large page support may be added in an opt-in manner in the [future].
///
/// [future]: https://webassembly.org/docs/future-features/#large-page-support
pub const WASM_PAGE_SIZE: u32 = 0x10000;

/// The number of pages we can have before we run out of byte index space.
pub const WASM_MAX_PAGES: u32 = 0x10000;
