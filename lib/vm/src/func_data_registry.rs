//! A registry for `VMFuncRef`s. This allows us to deduplicate funcrefs so that
//! identical `VMCallerCheckedAnyfunc`s will give us identical funcrefs.
//!
//! This registry also helps ensure that the `VMFuncRef`s can stay valid for as
//! long as we need them to.

use crate::vmcontext::VMCallerCheckedAnyfunc;
use loupe::MemoryUsage;

/// A function reference. A single word that points to metadata about a function.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, MemoryUsage)]
pub struct VMFuncRef(pub(crate) *const VMCallerCheckedAnyfunc);

impl wasmer_types::NativeWasmType for VMFuncRef {
    const WASM_TYPE: wasmer_types::Type = wasmer_types::Type::FuncRef;
    type Abi = Self;

    #[inline]
    fn from_abi(abi: Self::Abi) -> Self {
        abi
    }

    #[inline]
    fn into_abi(self) -> Self::Abi {
        self
    }

    #[inline]
    fn to_binary(self) -> i128 {
        self.0 as _
    }

    #[inline]
    fn from_binary(bits: i128) -> Self {
        // TODO: ensure that the safety invariants are actually upheld here
        Self(bits as _)
    }
}

impl VMFuncRef {
    /// Check if the FuncRef is null
    // TODO: make this const when `std::ptr::is_null` is const
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// Create a new null FuncRef
    pub const fn null() -> Self {
        Self(std::ptr::null())
    }
}

impl std::ops::Deref for VMFuncRef {
    type Target = *const VMCallerCheckedAnyfunc;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for VMFuncRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// We use raw pointers but the data never moves, so it's not a problem
// TODO: update docs
unsafe impl Send for VMFuncRef {}
unsafe impl Sync for VMFuncRef {}
