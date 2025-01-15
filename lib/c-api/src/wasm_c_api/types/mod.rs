mod export;
mod extern_;
mod frame;
mod function;
mod global;
mod import;
mod memory;
mod mutability;
mod table;
mod tag;
mod value;

pub use export::*;
pub use extern_::*;
pub use frame::*;
pub use function::*;
pub use global::*;
pub use import::*;
pub use memory::*;
pub use mutability::*;
pub use table::*;
use tag::*;
pub use value::*;

#[allow(non_camel_case_types)]
pub type wasm_byte_t = u8;

wasm_declare_vec!(byte);

#[allow(non_camel_case_types)]
pub type wasm_name_t = wasm_byte_vec_t;

impl From<String> for wasm_name_t {
    fn from(string: String) -> Self {
        string.into_bytes().into()
    }
}

// opaque type over `ExternRef`?
#[allow(non_camel_case_types)]
pub struct wasm_ref_t;

#[allow(non_camel_case_types)]
pub type wasm_message_t = wasm_byte_vec_t;
