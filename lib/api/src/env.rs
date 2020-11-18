use crate::{ExportError, Instance};
use thiserror::Error;

/// An error while initializing the user supplied host env with the `WasmerEnv` trait.
#[derive(Error, Debug)]
#[error("Host env initialization error: {0}")]
pub enum HostEnvInitError {
    /// An error occurred when accessing an export
    Export(ExportError),
}

impl From<ExportError> for HostEnvInitError {
    fn from(other: ExportError) -> Self {
        Self::Export(other)
    }
}

/// Prototype trait for finishing envs.
/// # Examples
///
/// This trait can be derived like so:
///
/// ```
/// # use wasmer::{WasmerEnv, LazyInit, Memory, NativeFunc};
///
/// #[derive(WasmerEnv)]
/// pub struct MyEnvWithNoInstanceData {
///     non_instance_data: u8,
/// }
///
/// #[derive(WasmerEnv)]
/// pub struct MyEnvWithInstanceData {
///     non_instance_data: u8,
///     #[wasmer(export)]
///     memory: LazyInit<Memory>,
///     #[wasmer(export(name = "real_name"))]
///     func: LazyInit<NativeFunc<(i32, i32), i32>>,
/// }
///
/// ```
///
/// This trait can also be implemented manually:
/// ```
/// # use wasmer::{WasmerEnv, LazyInit, Memory, Instance, HostEnvInitError};
/// pub struct MyEnv {
///    memory: LazyInit<Memory>,
/// }
///
/// impl WasmerEnv for MyEnv {
///     fn finish(&mut self, instance: &Instance) -> Result<(), HostEnvInitError> {
///         let memory = instance.exports.get_memory("memory").unwrap();
///         self.memory.initialize(memory.clone());
///         Ok(())
///     }
/// }
/// ```
pub trait WasmerEnv {
    /// The function that Wasmer will call on your type to let it finish
    /// setting up the environment with data from the `Instance`.
    ///
    /// This function is called after `Instance` is created but before it is
    /// returned to the user via `Instance::new`.
    fn finish(&mut self, instance: &Instance) -> Result<(), HostEnvInitError>;
}

macro_rules! impl_wasmer_env {
    ($name:ty) => {
        impl WasmerEnv for $name {
            fn finish(
                &mut self,
                _instance: &crate::Instance,
            ) -> Result<(), crate::HostEnvInitError> {
                Ok(())
            }
        }
    };
}

impl_wasmer_env!(u8);
impl_wasmer_env!(i8);
impl_wasmer_env!(u16);
impl_wasmer_env!(i16);
impl_wasmer_env!(u32);
impl_wasmer_env!(i32);
impl_wasmer_env!(u64);
impl_wasmer_env!(i64);
impl_wasmer_env!(u128);
impl_wasmer_env!(i128);
impl_wasmer_env!(f32);
impl_wasmer_env!(f64);
impl_wasmer_env!(usize);
impl_wasmer_env!(isize);
impl_wasmer_env!(char);
impl_wasmer_env!(bool);
impl_wasmer_env!(String);
impl_wasmer_env!(::std::sync::atomic::AtomicBool);
impl_wasmer_env!(::std::sync::atomic::AtomicI8);
impl_wasmer_env!(::std::sync::atomic::AtomicU8);
impl_wasmer_env!(::std::sync::atomic::AtomicI16);
impl_wasmer_env!(::std::sync::atomic::AtomicU16);
impl_wasmer_env!(::std::sync::atomic::AtomicI32);
impl_wasmer_env!(::std::sync::atomic::AtomicU32);
impl_wasmer_env!(::std::sync::atomic::AtomicI64);
impl_wasmer_env!(::std::sync::atomic::AtomicUsize);
impl_wasmer_env!(::std::sync::atomic::AtomicIsize);

impl<T: WasmerEnv> WasmerEnv for &'static mut T {
    fn finish(&mut self, instance: &Instance) -> Result<(), HostEnvInitError> {
        (*self).finish(instance)
    }
}

// TODO: do we want to use mutex/atomics here? like old WASI solution
/// Lazily init an item
pub struct LazyInit<T: Sized> {
    /// The data to be initialized
    data: std::mem::MaybeUninit<T>,
    /// Whether or not the data has been initialized
    initialized: bool,
}

impl<T> LazyInit<T> {
    /// Creates an unitialized value.
    pub fn new() -> Self {
        Self {
            data: std::mem::MaybeUninit::uninit(),
            initialized: false,
        }
    }

    /// # Safety
    /// - The data must be initialized first
    pub unsafe fn get_unchecked(&self) -> &T {
        &*self.data.as_ptr()
    }

    /// Get the inner data.
    pub fn get_ref(&self) -> Option<&T> {
        if !self.initialized {
            None
        } else {
            Some(unsafe { self.get_unchecked() })
        }
    }

    /// Sets a value and marks the data as initialized.
    pub fn initialize(&mut self, value: T) -> bool {
        if self.initialized {
            return false;
        }
        unsafe {
            self.data.as_mut_ptr().write(value);
        }
        self.initialized = true;
        true
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for LazyInit<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("LazyInit")
            .field("data", &self.get_ref())
            .finish()
    }
}

impl<T: Clone> Clone for LazyInit<T> {
    fn clone(&self) -> Self {
        if let Some(inner) = self.get_ref() {
            Self {
                data: std::mem::MaybeUninit::new(inner.clone()),
                initialized: true,
            }
        } else {
            Self {
                data: std::mem::MaybeUninit::uninit(),
                initialized: false,
            }
        }
    }
}

impl<T> Drop for LazyInit<T> {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                let ptr = self.data.as_mut_ptr();
                std::ptr::drop_in_place(ptr);
            };
        }
    }
}

impl<T> Default for LazyInit<T> {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<T: Send> Send for LazyInit<T> {}
// I thought we could opt out of sync..., look into this
// unsafe impl<T> !Sync for InitWithInstance<T> {}
