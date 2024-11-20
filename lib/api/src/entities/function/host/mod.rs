mod imp;

use crate::{
    vm::{VMFunctionCallback, VMTrampoline},
    WasmTypeList,
};

/// The `HostFunction` trait represents the set of functions that
/// can be used as host function. To uphold this statement, it is
/// necessary for a function to be transformed into a
/// `VMFunctionCallback`.
pub trait HostFunction<T, Args, Rets, Kind>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
    Kind: HostFunctionKind,
{
    #[cfg(feature = "js")]
    /// Get the pointer to the function body for a given runtime.
    fn js_function_callback(&self) -> crate::rt::js::vm::VMFunctionCallback;

    #[cfg(feature = "js")]
    /// Get the pointer to the function call trampoline for a given runtime.
    fn js_call_trampoline_address() -> crate::rt::js::vm::VMTrampoline {
        // This is not implemented in JS
        unimplemented!();
    }

    #[cfg(feature = "sys")]
    /// Get the pointer to the function body for a given runtime.
    fn sys_function_callback(&self) -> crate::rt::sys::vm::VMFunctionCallback;

    #[cfg(feature = "sys")]
    /// Get the pointer to the function call trampoline for a given runtime.
    fn sys_call_trampoline_address() -> crate::rt::sys::vm::VMTrampoline {
        // This is not implemented in JS
        unimplemented!();
    }

    #[cfg(feature = "wamr")]
    /// Get the pointer to the function body for a given runtime.
    fn wamr_function_callback(&self) -> crate::rt::wamr::vm::VMFunctionCallback;

    #[cfg(feature = "wamr")]
    /// Get the pointer to the function call trampoline for a given runtime.
    fn wamr_call_trampoline_address() -> crate::rt::wamr::vm::VMTrampoline {
        // This is not implemented in JS
        unimplemented!();
    }

    #[cfg(feature = "v8")]
    /// Get the pointer to the function body for a given runtime.
    fn v8_function_callback(&self) -> crate::rt::v8::vm::VMFunctionCallback;

    #[cfg(feature = "v8")]
    /// Get the pointer to the function call trampoline for a given runtime.
    fn v8_call_trampoline_address() -> crate::rt::v8::vm::VMTrampoline {
        // This is not implemented in JS
        unimplemented!();
    }
}

/// Empty trait to specify the kind of `HostFunction`: With or
/// without an environment.
///
/// This trait is never aimed to be used by a user. It is used by
/// the trait system to automatically generate the appropriate
/// host functions.
#[doc(hidden)]
pub trait HostFunctionKind: private::HostFunctionKindSealed {}

/// An empty struct to help Rust typing to determine
/// when a `HostFunction` does have an environment.
pub struct WithEnv;

impl HostFunctionKind for WithEnv {}

/// An empty struct to help Rust typing to determine
/// when a `HostFunction` does not have an environment.
pub struct WithoutEnv;

impl HostFunctionKind for WithoutEnv {}

mod private {
    //! Sealing the HostFunctionKind because it shouldn't be implemented
    //! by any type outside.
    //! See:
    //! <https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed>
    pub trait HostFunctionKindSealed {}
    impl HostFunctionKindSealed for super::WithEnv {}
    impl HostFunctionKindSealed for super::WithoutEnv {}
}
