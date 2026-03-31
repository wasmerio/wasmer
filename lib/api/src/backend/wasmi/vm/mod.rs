pub(crate) mod external;
mod env;
pub use external::*;
pub use env::*;

use super::entities::function::env::FunctionEnv;
use ::wasmi as wasmi_native;
use wasmer_types::RawValue;

pub use super::error::Trap;

pub(crate) type VMFunction = wasmi_native::Func;
pub(crate) type VMFunctionBody = ();
pub(crate) type VMFunctionCallback = *mut ::std::os::raw::c_void;
pub(crate) type VMTrampoline = *mut ::std::os::raw::c_void;
pub(crate) type VMExternFunction = wasmi_native::Func;

pub(crate) type VMGlobal = wasmi_native::Global;
pub(crate) type VMExternGlobal = wasmi_native::Global;

pub(crate) type VMMemory = wasmi_native::Memory;
pub type VMSharedMemory = VMMemory;
pub(crate) type VMExternMemory = wasmi_native::Memory;

pub(crate) type VMTable = wasmi_native::Table;
pub(crate) type VMExternTable = wasmi_native::Table;

pub(crate) type VMInstance = wasmi_native::Instance;

pub(crate) type VMException = ();
pub(crate) type VMTag = ();
pub(crate) type VMExternTag = ();
pub(crate) type VMExternObj = ();
pub(crate) type VMConfig = ();

pub(crate) struct VMFuncRef(());
impl VMFuncRef {
    pub fn into_raw(self) -> RawValue {
        let _ = self;
        unimplemented!()
    }

    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

pub struct VMExceptionRef(());
impl VMExceptionRef {
    pub fn into_raw(self) -> RawValue {
        let _ = self;
        unimplemented!()
    }

    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        unimplemented!();
    }
}

pub(crate) fn handle_bits<T: Copy>(value: T) -> u64 {
    debug_assert_eq!(core::mem::size_of::<T>(), core::mem::size_of::<u64>());
    unsafe { core::mem::transmute_copy::<T, u64>(&value) }
}
