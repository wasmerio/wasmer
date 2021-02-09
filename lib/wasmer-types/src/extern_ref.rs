use std::alloc::{self, Layout};
use std::any::Any;
use std::ptr;
use std::sync::atomic;

/// This type does not do reference counting automatically, reference counting can be done with
/// [`Self::ref_clone`] and [`Self::ref_drop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VMExternRef(*const VMExternRefInner);

impl VMExternRef {
    /// The maximum number of references allowed to this data.
    const MAX_REFCOUNT: usize = std::usize::MAX - 1;

    /// Checks if the given ExternRef is null.
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// New null extern ref
    pub const fn null() -> Self {
        Self(ptr::null())
    }

    /// Make a new extern reference
    pub fn new<T>(value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        Self(Box::into_raw(Box::new(VMExternRefInner::new::<T>(value))))
    }

    /// A deep copy of the reference, increments the strong count.
    pub fn ref_clone(&self) -> Self {
        if self.0.is_null() {
            return Self(self.0);
        }

        let old_size = unsafe {
            let ref_inner = &*self.0;
            ref_inner.increment_ref_count()
        };

        // However we need to guard against massive refcounts in case
        // someone is `mem::forget`ing `InstanceRef`. If we
        // don't do this the count can overflow and users will
        // use-after free. We racily saturate to `isize::MAX` on the
        // assumption that there aren't ~2 billion threads
        // incrementing the reference count at once. This branch will
        // never be taken in any realistic program.
        //
        // We abort because such a program is incredibly degenerate,
        // and we don't care to support it.

        if old_size > Self::MAX_REFCOUNT {
            panic!("Too many references of `InstanceRef`");
        }

        Self(self.0)
    }

    /// Does an inner drop, decrementing the strong count
    pub fn ref_drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let should_drop = {
                    let ref_inner: &VMExternRefInner = &*self.0;
                    ref_inner.decrement_and_drop()
                };
                if should_drop {
                    let _ = Box::from_raw(self.0 as *mut VMExternRefInner);
                }
            }
        }
    }
}

#[derive(Debug)]
#[repr(C)]
pub(crate) struct VMExternRefInner {
    strong: atomic::AtomicUsize,
    /// Do something obviously correct to get started. This can "easily" be improved
    /// to be an inline allocation later as the logic is fully encapsulated.
    data: Box<dyn Any + Send + Sync + 'static>,
}

impl VMExternRefInner {
    fn new<T>(value: T) -> Self
    where
        T: Any + Send + Sync + Sized + 'static,
    {
        Self {
            strong: atomic::AtomicUsize::new(1),
            data: Box::new(value),
        }
    }

    /// Returns the old value.
    /// TODO: document this
    fn increment_ref_count(&self) -> usize {
        // Using a relaxed ordering is alright here, as knowledge of
        // the original reference prevents other threads from
        // erroneously deleting the object.
        //
        // As explained in the [Boost documentation][1]:
        //
        // > Increasing the reference counter can always be done with
        // > `memory_order_relaxed`: New references to an object can
        // > only be formed from an existing reference, and passing an
        // > existing reference from one thread to another must already
        // > provide any required synchronization.
        //
        // [1]: https://www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html
        self.strong.fetch_add(1, atomic::Ordering::Relaxed)
    }

    /// Decrement the count and drop the data if the count hits 0
    /// returns `true` if the containing allocation should be dropped
    fn decrement_and_drop(&self) -> bool {
        // Because `fetch_sub` is already atomic, we do not need to
        // synchronize with other thread.
        if self.strong.fetch_sub(1, atomic::Ordering::Release) != 1 {
            return false;
        }

        // This fence is needed to prevent reordering of use of the data and
        // deletion of the data. Because it is marked `Release`, the decreasing
        // of the reference count synchronizes with this `Acquire` fence. This
        // means that use of the data happens before decreasing the reference
        // count, which happens before this fence, which happens before the
        // deletion of the data.
        //
        // As explained in the [Boost documentation][1]:
        //
        // > It is important to enforce any possible access to the object in one
        // > thread (through an existing reference) to *happen before* deleting
        // > the object in a different thread. This is achieved by a "release"
        // > operation after dropping a reference (any access to the object
        // > through this reference must obviously happened before), and an
        // > "acquire" operation before deleting the object.
        //
        // [1]: https://www.boost.org/doc/libs/1_55_0/doc/html/atomic/usage_examples.html
        atomic::fence(atomic::Ordering::Acquire);

        return true;
    }
}
