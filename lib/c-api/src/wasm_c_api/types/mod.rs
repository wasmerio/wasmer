mod export;
mod extern_;
mod function;
mod global;
mod import;
mod memory;
mod mutability;
mod table;
mod value;

pub use export::*;
pub use extern_::*;
pub use function::*;
pub use global::*;
pub use import::*;
pub use memory::*;
pub use mutability::*;
pub use table::*;
pub use value::*;

#[allow(non_camel_case_types)]
pub type wasm_byte_t = u8;

wasm_declare_vec!(byte);

#[derive(Debug)]
#[repr(C)]
pub struct wasm_frame_t {}

wasm_declare_vec!(frame);

/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub type wasm_name_t = wasm_byte_vec_t;

// opaque type over `ExternRef`?
/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub struct wasm_ref_t;
