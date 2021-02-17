use std::any::Any;
use std::marker::PhantomData;
use wasmer_vm::VMExternRef;
use crate::FromToNativeWasmType;

#[derive(Debug, Clone)]
#[repr(transparent)]
/// An opaque reference to some data. This reference can be passed through Wasm.
pub struct ExternRef<T: Any + Send + Sync + 'static + Sized> {
    inner: VMExternRef,
    _phantom: PhantomData<T>,
}

impl<T> ExternRef<T>
where
    T: Any + Send + Sync + 'static + Sized,
{
    /// Checks if the given ExternRef is null.
    pub fn is_null(&self) -> bool {
        self.inner.is_null()
    }

    /// New null extern ref
    pub fn null() -> Self {
        Self {
            inner: VMExternRef::null(),
            _phantom: PhantomData,
        }
    }

    /// Make a new extern reference
    pub fn new(value: T) -> Self {
        Self {
            inner: VMExternRef::new(value),
            _phantom: PhantomData,
        }
    }

    /// Try to downcast to the given value
    pub fn downcast(&self) -> Option<&T> {
        self.inner.downcast::<T>()
    }
}

unsafe impl<T> FromToNativeWasmType for ExternRef<T>
where T: Any + Send + Sync + 'static + Sized,
{
    type Native = VMExternRef;

    fn to_native(self) -> Self::Native {
        self.inner
    }
    fn from_native(n: Self::Native) -> Self {
        Self {
            inner: n,
            _phantom: PhantomData,
        }
    }
}
