use std::{
    error::Error,
    ffi::{c_char, CStr},
};

use crate::{wasmi::bindings::*, AsStoreMut};

#[derive(Debug)]
enum InnerTrap {
    User(Box<dyn Error + Send + Sync>),
    CApi(*mut wasm_trap_t),
}

/// A struct representing a Trap
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

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<T, Self> {
        match self.inner {
            // We only try to downcast user errors
            InnerTrap::User(err) if err.is::<T>() => Ok(*err.downcast::<T>().unwrap()),
            _ => Err(self),
        }
    }

    /// Attempts to downcast the `Trap` to a concrete type.
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        match &self.inner {
            // We only try to downcast user errors
            InnerTrap::User(err) if err.is::<T>() => err.downcast_ref::<T>(),
            _ => None,
        }
    }

    /// Returns true if the `Trap` is the same as T
    pub fn is<T: Error + 'static>(&self) -> bool {
        match &self.inner {
            InnerTrap::User(err) => err.is::<T>(),
            _ => false,
        }
    }

    pub unsafe fn into_wasm_trap(self, store: &mut impl AsStoreMut) -> *mut wasm_trap_t {
        match self.inner {
            InnerTrap::CApi(t) => t,
            InnerTrap::User(err) => {
                let err_ptr = Box::leak(Box::new(err));
                let mut data = std::mem::zeroed();
                // let x = format!("")
                let s1 = format!("🐛{:p}", err_ptr);
                let _s = s1.into_bytes().into_boxed_slice();
                wasm_byte_vec_new(&mut data, _s.len(), _s.as_ptr() as _);
                std::mem::forget(_s);
                let store = store.as_store_mut();
                wasm_trap_new(store.inner.store.as_wasmi().inner, &mut data)
            }
        }
    }

    // pub unsafe fn deserialize_from_wasm_trap(trap: *mut wasm_trap_t) -> Self {
    //     let mut data = std::mem::zeroed();
    //     wasm_trap_message(trap, data);
    //     println!("data: {:p}", data);

    //     std::ptr::read(data as *const _)
    // }
}

impl From<*mut wasm_trap_t> for Trap {
    fn from(value: *mut wasm_trap_t) -> Self {
        let message = unsafe {
            let mut message = std::mem::zeroed();
            wasm_trap_message(value, &mut message);

            CStr::from_ptr(message.data as *const c_char)
                .to_str()
                .unwrap()
        };

        println!("{message}");

        if message.starts_with("Exception: 🐛") {
            let ptr_str = message.replace("Exception: 🐛", "");
            let ptr: Box<dyn Error + Send + Sync + 'static> = unsafe {
                let r = ptr_str.trim_start_matches("0x");
                std::ptr::read(
                    (usize::from_str_radix(&r, 16).unwrap()
                        as *const Box<dyn Error + Send + Sync + 'static>),
                )
            };

            Self {
                inner: InnerTrap::User(ptr),
            }
        } else {
            Self {
                inner: InnerTrap::CApi(value),
            }
        }
    }
}

impl std::error::Error for Trap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.inner {
            InnerTrap::User(err) => Some(&**err),
            _ => None,
        }
    }
}

impl std::fmt::Display for Trap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            InnerTrap::User(e) => write!(f, "{}", e),
            InnerTrap::CApi(value) => {
                // let message: wasm_message_t;
                // wasm_trap_message(value, &mut message);
                let mut out = unsafe {
                    let mut vec: wasm_byte_vec_t = Default::default();
                    wasm_byte_vec_new_empty(&mut vec);
                    &mut vec as *mut _
                };
                unsafe { wasm_trap_message(*value, out) };
                let cstr = unsafe { CStr::from_ptr((*out).data) };
                write!(f, "wasm-c-api trap: {}", cstr.to_str().unwrap())
            }
        }
    }
}

impl std::fmt::Debug for Trap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            InnerTrap::User(e) => write!(f, "{}", e),
            InnerTrap::CApi(value) => {
                // let message: wasm_message_t;
                // wasm_trap_message(value, &mut message);
                let mut out = unsafe {
                    let mut vec: wasm_byte_vec_t = Default::default();
                    wasm_byte_vec_new_empty(&mut vec);
                    &mut vec as *mut _
                };
                unsafe { wasm_trap_message(*value, out) };
                let cstr = unsafe { CStr::from_ptr((*out).data) };
                write!(f, "wasm-c-api trap: {}", cstr.to_str().unwrap())
            }
        }
    }
}

impl From<Trap> for crate::RuntimeError {
    fn from(trap: Trap) -> Self {
        if trap.is::<Self>() {
            return trap.downcast::<Self>().unwrap();
        }

        crate::RuntimeError::new_from_source(crate::BackendTrap::Wasmi(trap), vec![], None)
    }
}
