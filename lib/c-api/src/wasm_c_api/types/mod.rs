mod export;
mod extern_;
mod function;
mod global;
mod import;
mod memory;
mod mutability;
mod name;
mod reference;
mod table;
mod value;

pub use export::*;
pub use extern_::*;
pub use function::*;
pub use global::*;
pub use import::*;
pub use memory::*;
pub use mutability::*;
pub use name::*;
pub use reference::*;
pub use table::*;
pub use value::*;

#[allow(non_camel_case_types)]
pub type wasm_byte_t = u8;

wasm_declare_vec!(byte);

#[derive(Debug)]
#[repr(C)]
pub struct wasm_frame_t {}

wasm_declare_vec!(frame);
