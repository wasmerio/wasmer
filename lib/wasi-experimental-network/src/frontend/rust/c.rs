use crate::types::{__wasi_errno_t, __WASI_ESUCCESS};
use std::io;

pub trait CResult {
    type Target;

    fn is_success(&self) -> bool;
    fn into_result(&self) -> io::Result<Self::Target>;
}

macro_rules! impl_c_result {
    ( $($t:ident),* ) => {
        $(
            impl CResult for $t {
                type Target = Self;

                fn is_success(&self) -> bool {
                    *self == __WASI_ESUCCESS
                }

                fn into_result(&self) -> io::Result<Self::Target> {
                    if self.is_success() {
                        Ok(*self)
                    } else {
                        Err(io::Error::last_os_error())
                    }
                }
            }
        )*
    };
}

impl_c_result! { __wasi_errno_t }
