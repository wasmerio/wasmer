pub(crate) mod external;
pub(crate) mod function;
pub(crate) mod global;
pub(crate) mod memory;
pub(crate) mod table;
pub use super::error::Trap;

pub use external::*;
pub use function::*;
pub use global::*;
pub use memory::*;
pub use table::*;

/// The type of instances in the `js` VM.
pub type VMInstance = rusty_jsc::JSObject;

pub struct VMTrampoline;

/// The type of extern tables in the `js` VM.
pub(crate) type VMExternTable = VMTable;
/// The type of extern memories in the `js` VM.
pub(crate) type VMExternMemory = VMMemory;
/// The type of extern globals in the `js` VM.
pub(crate) type VMExternGlobal = VMGlobal;
/// The type of extern functions in the `js` VM.
pub(crate) type VMExternFunction = VMFunction;

// No EH for now.
pub(crate) type VMException = ();
pub(crate) type VMTag = ();
pub(crate) type VMExternTag = ();

pub struct VMExceptionRef;

impl VMExceptionRef {
    /// Converts the `VMExceptionRef` into a `RawValue`.
    pub fn into_raw(self) -> wasmer_types::RawValue {
        unimplemented!()
    }

    /// Extracts a `VMExceptionRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExceptionRef` instance.
    pub unsafe fn from_raw(_raw: wasmer_types::RawValue) -> Option<Self> {
        unimplemented!();
    }
}
