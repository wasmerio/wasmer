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

impl AsRef<wasm_name_t> for wasm_name_t {
    fn as_ref(&self) -> &wasm_name_t {
        &self
    }
}

/// An owned version of `wasm_name_t`.
///
/// Assumes that data is either valid host-owned or null.
// NOTE: `wasm_name_t` already does a deep copy, so we just derive `Clone` here.
#[derive(Debug, Clone)]
#[repr(transparent)]
#[allow(non_camel_case_types)]
pub struct owned_wasm_name_t(wasm_name_t);

impl owned_wasm_name_t {
    /// Take ownership of some `wasm_name_t`
    ///
    /// # Safety
    /// You must ensure that the data pointed to by `wasm_name_t` is valid and
    /// that it is not owned by anyone else.
    pub unsafe fn new(name: &wasm_name_t) -> Self {
        Self(wasm_name_t {
            size: name.size,
            data: name.data,
        })
    }
}

impl Drop for owned_wasm_name_t {
    fn drop(&mut self) {
        if !self.0.data.is_null() {
            let _v = unsafe { Vec::from_raw_parts(self.0.data, self.0.size, self.0.size) };
            self.0.data = std::ptr::null_mut();
            self.0.size = 0;
        }
        // why can't we call this function?
        //unsafe { crate::wasm_c_api::macros::wasm_byte_vec_delete(Some(self.0)) }
    }
}

impl AsRef<wasm_name_t> for owned_wasm_name_t {
    fn as_ref(&self) -> &wasm_name_t {
        &self.0
    }
}

impl From<String> for owned_wasm_name_t {
    fn from(string: String) -> Self {
        let mut boxed_str: Box<str> = string.into_boxed_str();
        let data = boxed_str.as_mut_ptr();
        let size = boxed_str.bytes().len();
        let wasm_name = wasm_name_t { data, size };

        Box::leak(boxed_str);

        Self(wasm_name)
    }
}

// opaque type over `ExternRef`?
#[allow(non_camel_case_types)]
pub struct wasm_ref_t;

#[allow(non_camel_case_types)]
pub type wasm_message_t = wasm_byte_vec_t;
