use std::error::Error;

use ::wasmi;

use crate::wasmi::vm::VMExceptionRef;

#[derive(Debug)]
struct UserTrap(Box<dyn Error + Send + Sync>);

impl std::fmt::Display for UserTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for UserTrap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.0)
    }
}

impl wasmi::errors::HostError for UserTrap {}

#[derive(Debug)]
enum InnerTrap {
    User(Box<dyn Error + Send + Sync>),
    Wasmi(wasmi::Error),
}

/// A struct representing a Trap.
#[derive(Debug)]
pub struct Trap {
    inner: InnerTrap,
}

unsafe impl Send for Trap {}
unsafe impl Sync for Trap {}

impl Trap {
    pub fn user(error: Box<dyn Error + Send + Sync>) -> Self {
        Self {
            inner: InnerTrap::User(error),
        }
    }

    pub(crate) fn from_wasmi_error(error: wasmi::Error) -> crate::RuntimeError {
        Self {
            inner: InnerTrap::Wasmi(error),
        }
        .into()
    }

    pub(crate) fn into_wasmi_error(self) -> wasmi::Error {
        match self.inner {
            InnerTrap::User(err) => wasmi::Error::host(UserTrap(err)),
            InnerTrap::Wasmi(err) => err,
        }
    }

    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match self.inner {
            InnerTrap::User(err) if err.is::<T>() => Ok(*err.downcast::<T>().unwrap()),
            inner => Err(Self { inner }),
        }
    }

    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        match &self.inner {
            InnerTrap::User(err) if err.is::<T>() => err.downcast_ref::<T>(),
            _ => None,
        }
    }

    pub fn is<T: Error + 'static>(&self) -> bool {
        self.downcast_ref::<T>().is_some()
    }

    pub fn is_exception(&self) -> bool {
        false
    }

    pub fn to_exception_ref(&self) -> Option<VMExceptionRef> {
        None
    }
}

impl std::error::Error for Trap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.inner {
            InnerTrap::User(err) => Some(&**err),
            InnerTrap::Wasmi(err) => Some(err),
        }
    }
}

impl std::fmt::Display for Trap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            InnerTrap::User(err) => write!(f, "{err}"),
            InnerTrap::Wasmi(err) => write!(f, "{err}"),
        }
    }
}

impl From<Trap> for crate::RuntimeError {
    fn from(trap: Trap) -> Self {
        if trap.is::<Self>() {
            return trap.downcast::<Self>().unwrap();
        }

        Self::new_from_source(crate::BackendTrap::Wasmi(trap), vec![], None)
    }
}
