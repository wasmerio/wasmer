use std::any::Any;

use wasmer_vm::{ContextHandle, VMExternObj, VMExternRef};

use super::context::{AsContextMut, AsContextRef};

#[derive(Debug, Clone)]
#[repr(transparent)]
/// An opaque reference to some data. This reference can be passed through Wasm.
pub struct ExternRef {
    handle: ContextHandle<VMExternObj>,
}

impl ExternRef {
    /// Make a new extern reference
    pub fn new<T>(ctx: &mut impl AsContextMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        Self {
            handle: ContextHandle::new(ctx.as_context_mut().objects_mut(), VMExternObj::new(value)),
        }
    }

    /// Try to downcast to the given value.
    pub fn downcast<'a, T>(&self, ctx: &'a impl AsContextRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        self.handle
            .get(ctx.as_context_ref().objects())
            .as_ref()
            .downcast_ref::<T>()
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        VMExternRef(self.handle.internal_handle())
    }

    pub(crate) unsafe fn from_vm_externref(
        ctx: &mut impl AsContextMut,
        vm_externref: VMExternRef,
    ) -> Self {
        Self {
            handle: ContextHandle::from_internal(
                ctx.as_context_mut().objects_mut().id(),
                vm_externref.0,
            ),
        }
    }

    /// Checks whether this `ExternRef` can be used with the given context.
    ///
    /// Primitive (`i32`, `i64`, etc) and null funcref/externref values are not
    /// tied to a context and can be freely shared between contexts.
    ///
    /// Externref and funcref values are tied to a context and can only be used
    /// with that context.
    pub fn is_from_context(&self, ctx: &impl AsContextRef) -> bool {
        self.handle.context_id() == ctx.as_context_ref().objects().id()
    }
}
