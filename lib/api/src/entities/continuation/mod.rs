pub(crate) mod inner;
pub(crate) use inner::*;
use wasmer_types::{TagType, Type};

use crate::{
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, Tag, Value,
    vm::{VMContinuationRef, VMExtern, VMExternTag},
};

/// A WebAssembly `continuation` instance.
///
/// An continuation is an internal construct in WebAssembly that represents a runtime object that can
/// be thrown. A WebAssembly continuation consists of an continuation tag and its runtime arguments.
///
/// Spec: <https://github.com/WebAssembly/continuation-handling/blob/main/proposals/continuation-handling/Continuations.md#continuations>
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Continuation(pub(crate) BackendContinuation);

impl Continuation {
    /// Create a new continuation with the given tag and payload, and also creates
    /// a reference to it, returning the reference.
    pub fn new(store: &mut impl AsStoreMut, tag: &Tag, payload: &[Value]) -> Self {
        Self(BackendContinuation::new(store, tag, payload))
    }

    /// Check whether this `Continuation` comes from the given store.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store)
    }

    /// Get the continuation tag.
    pub fn tag(&self, store: &impl AsStoreRef) -> Tag {
        self.0.tag(store)
    }

    /// Get the continuation payload values.
    pub fn payload(&self, store: &mut impl AsStoreMut) -> Vec<Value> {
        self.0.payload(store)
    }

    /// Get the `VMContinuationRef` corresponding to this `Continuation`.
    pub fn vm_continuationref(&self) -> VMContinuationRef {
        self.0.vm_continuationref()
    }

    /// Create an `Continuation` from a `VMContinuationRef`.
    pub fn from_vm_continuationref(exnref: VMContinuationRef) -> Self {
        Self(BackendContinuation::from_vm_continuationref(exnref))
    }
}
