use crate::{AsStoreMut, Extern, RuntimeError, Value};
use wasmer_types::RawValue;

macro_rules! stub_struct {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name;

        impl $name {
            pub fn stub() -> Self {
                Self
            }
        }
    };
}

stub_struct!(VMExtern);
stub_struct!(VMExternFunction);
stub_struct!(VMExternGlobal);
stub_struct!(VMExternTag);
stub_struct!(VMExternMemory);
stub_struct!(VMExternTable);
stub_struct!(VMFunctionCallback);
stub_struct!(VMFunctionBody);
stub_struct!(VMFunctionEnvironment);
stub_struct!(VMInstance);
stub_struct!(VMTrampoline);
stub_struct!(VMFunction);
stub_struct!(VMGlobal);
stub_struct!(VMTag);
stub_struct!(VMException);
stub_struct!(VMMemory);
stub_struct!(VMSharedMemory);
stub_struct!(VMTable);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VMFuncRef {
    raw: usize,
}

impl VMFuncRef {
    pub fn stub() -> Self {
        Self { raw: 0 }
    }

    pub fn into_raw(self) -> RawValue {
        RawValue { funcref: self.raw }
    }

    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        Some(Self::stub())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VMExternRef {
    raw: usize,
}

impl VMExternRef {
    pub fn stub() -> Self {
        Self { raw: 0 }
    }

    pub fn into_raw(self) -> RawValue {
        RawValue { externref: self.raw }
    }

    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        Some(Self::stub())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VMExceptionRef {
    raw: usize,
}

impl VMExceptionRef {
    pub fn stub() -> Self {
        Self { raw: 0 }
    }

    pub fn into_raw(self) -> RawValue {
        RawValue { externref: self.raw }
    }

    pub unsafe fn from_raw(_raw: RawValue) -> Option<Self> {
        Some(Self::stub())
    }
}

impl VMExtern {
    pub fn to_extern(self, _store: &mut impl AsStoreMut) -> Extern {
        panic!("The stub backend cannot materialize VM externs")
    }
}

impl VMExternFunction {
    pub fn call(
        &self,
        _store: &mut impl AsStoreMut,
        _params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        Err(RuntimeError::new(
            "The stub backend cannot execute VM functions",
        ))
    }
}


#[derive(Clone, Copy, Debug, Default)]
pub struct Trap;

impl Trap {
    pub fn user(error: Box<dyn std::error::Error + Send + Sync>) -> crate::RuntimeError {
        crate::RuntimeError::new(format!("stub backend trap: {}", error))
    }

    pub fn downcast<T: std::error::Error + 'static>(self) -> Result<T, Self> {
        panic!("stub backend does not support trap downcasting")
    }

    pub fn downcast_ref<T: std::error::Error + 'static>(&self) -> Option<&T> {
        None
    }

    pub fn is<T: std::error::Error + 'static>(&self) -> bool {
        false
    }
}

impl std::fmt::Display for Trap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stub trap")
    }
}

impl std::error::Error for Trap {}

impl From<Trap> for crate::RuntimeError {
    fn from(_value: Trap) -> Self {
        crate::RuntimeError::new("stub backend trap")
    }
}
