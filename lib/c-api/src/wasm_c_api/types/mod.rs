mod export;
mod extern_;
mod frame;
mod function;
mod global;
mod import;
mod memory;
mod mutability;
mod table;
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
pub use value::*;

#[allow(non_camel_case_types)]
pub type wasm_byte_t = u8;

wasm_declare_vec!(byte);

#[allow(non_camel_case_types)]
pub type wasm_name_t = wasm_byte_vec_t;

impl From<String> for wasm_name_t {
    fn from(string: String) -> Self {
        let mut boxed_str: Box<str> = string.into_boxed_str();
        let data = boxed_str.as_mut_ptr();
        let size = boxed_str.bytes().len();
        let wasm_byte = Self { data, size };

        Box::leak(boxed_str);

        wasm_byte
    }
}

// opaque type over `ExternRef`?
#[allow(non_camel_case_types)]
pub struct wasm_ref_t;

#[allow(non_camel_case_types)]
pub type wasm_message_t = wasm_byte_vec_t;
