use super::*;

impl VMExternToExtern for VMExtern {
    fn to_extern(self, store: &mut impl crate::AsStoreMut) -> crate::Extern {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(s) => s.to_extern(store),
            #[cfg(feature = "wamr")]
            Self::Wamr(v) => v.to_extern(store),
            #[cfg(feature = "v8")]
            Self::V8(v) => v.to_extern(store),
        }
    }
}

impl VMFunctionEnvironment {
    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn std::any::Any + Send + 'static) {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(r) => r.as_ref(),
            #[cfg(feature = "wamr")]
            Self::Wamr(r) => r.as_ref(),
            #[cfg(feature = "v8")]
            Self::V8(r) => r.as_ref(),
            _ => panic!("No runtime enabled!"),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn std::any::Any + Send + 'static) {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(r) => r.as_mut(),
            #[cfg(feature = "wamr")]
            Self::Wamr(r) => r.as_mut(),
            #[cfg(feature = "v8")]
            Self::V8(r) => r.as_mut(),
            _ => panic!("No runtime enabled!"),
        }
    }

    pub fn contents(self) -> Box<(dyn std::any::Any + Send + 'static)> {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(r) => r.contents,
            #[cfg(feature = "wamr")]
            Self::Wamr(r) => r.contents,
            #[cfg(feature = "v8")]
            Self::V8(r) => r.contents,
            _ => panic!("No runtime enabled!"),
        }
    }
}

impl VMFuncRef {
    /// Converts the `VMFuncRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(r) => r.into_raw(),
            #[cfg(feature = "wamr")]
            Self::Wamr(r) => r.into_raw(),
            #[cfg(feature = "v8")]
            Self::V8(r) => r.into_raw(),
            _ => panic!("No runtime enabled!"),
        }
    }
}

impl VMExternRef {
    /// Converts the `VMExternRef` into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(r) => r.into_raw(),
            #[cfg(feature = "wamr")]
            Self::Wamr(r) => r.into_raw(),
            #[cfg(feature = "v8")]
            Self::V8(r) => r.into_raw(),
            _ => panic!("No runtime enabled!"),
        }
    }
}
