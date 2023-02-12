#[cfg(feature = "js")]
pub use crate::js::externals::function::{
    FromToNativeWasmType, Function, HostFunction, WasmTypeList,
};
#[cfg(feature = "sys")]
pub use crate::sys::externals::function::{
    FromToNativeWasmType, Function, HostFunction, WasmTypeList,
};
