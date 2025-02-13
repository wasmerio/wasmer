mod imp;

use crate::{
    vm::{VMFunctionCallback, VMTrampoline},
    BackendKind, WasmTypeList,
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
    /// Get the pointer to the function body for a given runtime.
    fn function_callback(&self, rt: BackendKind) -> crate::vm::VMFunctionCallback;

    /// Get the pointer to the function call trampoline for a given runtime.
    fn call_trampoline_address(rt: BackendKind) -> crate::vm::VMTrampoline;
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
