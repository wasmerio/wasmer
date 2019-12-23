//! Unique identifiers.

use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Once,
};

/// The backing structure providing configuration and storage for a UniqueId.
pub trait UniqueIdBacking {
    /// Returns the maximum amount of unique ids.
    fn get_size() -> usize;

    /// Returns the backing counter for the unique id.
    fn get_counter() -> &'static AtomicUsize;
}

/// Defines a unique ID category.
#[macro_export]
macro_rules! define_unique_id_category {
    ($name:ident, $max:expr) => {
        /// Automatically generated unique ID category.
        pub struct $name;
        impl $crate::unique_id::UniqueIdBacking for $name {
            fn get_size() -> usize {
                $max
            }

            fn get_counter() -> &'static ::std::sync::atomic::AtomicUsize {
                static COUNTER: ::std::sync::atomic::AtomicUsize =
                    ::std::sync::atomic::AtomicUsize::new(0);
                &COUNTER
            }
        }
    };
}

/// A unique identifier.
pub struct UniqueId<B: UniqueIdBacking> {
    /// Init once field.
    init: Once,
    /// Inner field.
    inner: UnsafeCell<usize>,
    _phantom: PhantomData<B>,
}

unsafe impl<B: UniqueIdBacking> Send for UniqueId<B> {}
unsafe impl<B: UniqueIdBacking> Sync for UniqueId<B> {}

impl<B: UniqueIdBacking> UniqueId<B> {
    /// Allocate and return a `UniqueId`.
    pub const fn allocate() -> UniqueId<B> {
        UniqueId {
            init: Once::new(),
            inner: UnsafeCell::new(::std::usize::MAX),
            _phantom: PhantomData,
        }
    }

    /// Get the index of this `UniqueId`.
    pub fn index(&self) -> usize {
        let inner: *mut usize = self.inner.get();
        self.init.call_once(|| {
            let counter = B::get_counter();
            let idx = counter.fetch_add(1, Ordering::SeqCst);
            if idx >= B::get_size() {
                counter.fetch_sub(1, Ordering::SeqCst);
                panic!("at most {} unique IDs are supported", B::get_size());
            } else {
                unsafe {
                    *inner = idx;
                }
            }
        });
        unsafe { *inner }
    }
}
