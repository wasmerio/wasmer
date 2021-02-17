use crate::FromToNativeWasmType;
use std::any::Any;
use std::marker::PhantomData;
use wasmer_vm::VMExternRef;

#[derive(Debug)]
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

/*
unsafe impl<T> FromToNativeWasmType for ExternRef<T>
where
    T: Any + Send + Sync + 'static + Sized,
{
    type Native = usize;

    #[inline]
    fn from_native(native: Self::Native) -> Self {
        let inner = VMExternRef::from_ne_bytes(Self::Native::to_ne_bytes(native));
        Self {
            inner,
            _phantom: PhantomData,
        }
    }

    #[inline]
    fn to_native(self) -> Self::Native {
        Self::Native::from_ne_bytes(VMExternRef::to_ne_bytes(self.inner))
    }
}
*/
