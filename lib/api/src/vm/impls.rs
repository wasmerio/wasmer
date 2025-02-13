use crate::macros::backend::match_rt;

use super::*;

impl VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl crate::AsStoreMut) -> crate::Extern {
        match_rt!(on self => s {
            s.to_extern(store)
        })
    }
}

impl VMFunctionEnvironment {
    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn std::any::Any + Send + 'static) {
        match_rt!(on self => s {
            s.as_ref()
        })
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn std::any::Any + Send + 'static) {
        match_rt!(on self => s {
            s.as_mut()
        })
    }

    pub fn contents(self) -> Box<(dyn std::any::Any + Send + 'static)> {
        match_rt!(on self => s {
            s.contents
        })
    }
}

impl VMFuncRef {
    /// Converts the `VMFuncRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        match_rt!(on self => s {
            s.into_raw()
        })
    }
}

impl VMExternRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        match_rt!(on self => s {
            s.into_raw()
        })
    }
}

impl VMExceptionRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        todo!()
    }
}
