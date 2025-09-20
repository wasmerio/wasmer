pub(crate) mod inner;
pub(crate) use inner::*;
use wasmer_types::{TagType, Type};

use crate::{
    vm::{VMExceptionRef, VMExtern, VMExternTag},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, Tag, Value,
};

/// A WebAssembly `exception` instance.
///
/// An exception is an internal construct in WebAssembly that represents a runtime object that can
/// be thrown. A WebAssembly exception consists of an exception tag and its runtime arguments.
///
/// Spec: <https://github.com/WebAssembly/exception-handling/blob/main/proposals/exception-handling/Exceptions.md#exceptions>
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Exception(pub(crate) BackendException);

impl Exception {
    /// Create a new exception with the given tag and payload, and also creates
    /// a reference to it, returning the reference.
    pub fn new(store: &mut impl AsStoreMut, tag: Tag, payload: &[Value]) -> Self {
        Self(BackendException::new(store, tag, payload))
    }

    /// Check whether this `Exception` comes from the given store.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    /// Get the exception tag.
    pub fn tag(&self, store: &impl AsStoreRef) -> Tag {
        self.0.tag(store)
    }

    /// Get the exception payload values.
    pub fn payload(&self, store: &mut impl AsStoreMut) -> Vec<Value> {
        self.0.payload(store)
    }

    /// Get the `VMExceptionRef` corresponding to this `Exception`.
    pub fn vm_exceptionref(&self) -> VMExceptionRef {
        self.0.vm_exceptionref()
    }

    /// Create an `Exception` from a `VMExceptionRef`.
    pub fn from_vm_exceptionref(exnref: VMExceptionRef) -> Self {
        Self(BackendException::from_vm_exceptionref(exnref))
    }
}
