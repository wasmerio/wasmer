use crate::types::__wasi_errno_t;
use std::io;

pub trait CResult {
    type Target;

    fn is_zero(&self) -> bool;
    fn into_result(&self) -> io::Result<Self::Target>;
}

macro_rules! impl_c_result {
    ( $($t:ident),* ) => {
        $(
            impl CResult for $t {
                type Target = Self;

                fn is_zero(&self) -> bool {
                    *self == 0
                }

                fn into_result(&self) -> io::Result<Self::Target> {
                    if self.is_zero() {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(*self)
                    }
                }
            }
        )*
    };
}

impl_c_result! { __wasi_errno_t }
