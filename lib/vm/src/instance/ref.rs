use super::Instance;
use loupe::{MemoryUsage, MemoryUsageTracker};
use std::alloc::Layout;
use std::mem;
use std::ptr::{self, NonNull};
use std::sync::{atomic, Arc};

/// An `InstanceRef` is responsible to properly deallocate,
/// and to give access to an `Instance`, in such a way that `Instance`
/// is unique, can be shared, safely, across threads, without
/// duplicating the pointer in multiple locations. `InstanceRef`
/// must be the only “owner” of an `Instance`.
///
/// Consequently, one must not share `Instance` but
/// `InstanceRef`. It acts like an Atomically Reference Counter
/// to `Instance`. In short, `InstanceRef` is roughly a
/// simplified version of `std::sync::Arc`.
///
/// This `InstanceRef` must be freed with [`InstanceRef::deallocate_instance`]
/// if and only if it has been set correctly. The `Drop` implementation of
/// [`InstanceRef`] calls its `deallocate_instance` method without
/// checking if this  property holds, only when `Self.strong` is equal to 1.
///
/// Note for the curious reader: [`InstanceAllocator::new`]
/// and [`InstanceHandle::new`] will respectively allocate a proper
/// `Instance` and will fill it correctly.
///
/// A little bit of background: The initial goal was to be able to
/// share an [`Instance`] between an [`InstanceHandle`] and the module
/// exports, so that one can drop a [`InstanceHandle`] but still being
/// able to use the exports properly.
///
/// This structure has a C representation because `Instance` is
/// dynamically-sized, and the `instance` field must be last.
#[derive(Debug)]
#[repr(C)]
pub struct InstanceRef {
    /// Number of `Self` in the nature. It increases when `Self` is
    /// cloned, and it decreases when `Self` is dropped.
    strong: Arc<atomic::AtomicUsize>,

    /// The layout of `Instance` (which can vary).
    instance_layout: Layout,

    /// The `Instance` itself. It must be the last field of
    /// `InstanceRef` since `Instance` is dyamically-sized.
    ///
    /// `Instance` must not be dropped manually by Rust, because it's
    /// allocated manually with `alloc` and a specific layout (Rust
    /// would be able to drop `Instance` itself but it will imply a
    /// memory leak because of `alloc`).
    ///
    /// No one in the code has a copy of the `Instance`'s
    /// pointer. `Self` is the only one.
    instance: NonNull<Instance>,
}

impl InstanceRef {
    /// Create a new `InstanceRef`. It allocates nothing. It fills
    /// nothing. The `Instance` must be already valid and
    /// filled.
    ///
    /// # Safety
    ///
    /// `instance` must a non-null, non-dangling, properly aligned,
    /// and correctly initialized pointer to `Instance`. See
    /// [`InstanceAllocator`] for an example of how to correctly use
    /// this API.
    pub(super) unsafe fn new(instance: NonNull<Instance>, instance_layout: Layout) -> Self {
        Self {
            strong: Arc::new(atomic::AtomicUsize::new(1)),
            instance_layout,
            instance,
        }
    }

    /// A soft limit on the amount of references that may be made to an `InstanceRef`.
    ///
    /// Going above this limit will make the program to panic at exactly
    /// `MAX_REFCOUNT` references.
    const MAX_REFCOUNT: usize = std::usize::MAX - 1;

    /// Deallocate `Instance`.
    ///
    /// # Safety
    ///
    /// `Self.instance` must be correctly set and filled before being
    /// dropped and deallocated.
    unsafe fn deallocate_instance(&mut self) {
        let instance_ptr = self.instance.as_ptr();

        ptr::drop_in_place(instance_ptr);
        std::alloc::dealloc(instance_ptr as *mut u8, self.instance_layout);
    }

    /// Get the number of strong references pointing to this
    /// `InstanceRef`.
    pub fn strong_count(&self) -> usize {
        self.strong.load(atomic::Ordering::SeqCst)
    }

    /// Get a reference to the `Instance`.
    #[inline]
    pub(crate) fn as_ref(&self) -> &Instance {
        // SAFETY: The pointer is properly aligned, it is
        // “dereferencable”, it points to an initialized memory of
        // `Instance`, and the reference has the lifetime `'a`.
        unsafe { self.instance.as_ref() }
    }

    #[inline]
    pub(super) unsafe fn as_mut(&mut self) -> &mut Instance {
        self.instance.as_mut()
    }
}

/// TODO: Review this super carefully.
unsafe impl Send for InstanceRef {}
unsafe impl Sync for InstanceRef {}

impl Clone for InstanceRef {
    /// Makes a clone of `InstanceRef`.
    ///
    /// This creates another `InstanceRef` using the same
    /// `instance` pointer, increasing the strong reference count.
    #[inline]
    fn clone(&self) -> Self {
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
        let old_size = self.strong.fetch_add(1, atomic::Ordering::Relaxed);

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

        Self {
            strong: self.strong.clone(),
            instance_layout: self.instance_layout,
            instance: self.instance.clone(),
        }
    }
}

impl PartialEq for InstanceRef {
    /// Two `InstanceRef` are equal if and only if
    /// `Self.instance` points to the same location.
    fn eq(&self, other: &Self) -> bool {
        self.instance == other.instance
    }
}

impl Drop for InstanceRef {
    /// Drop the `InstanceRef`.
    ///
    /// This will decrement the strong reference count. If it reaches
    /// 1, then the `Self.instance` will be deallocated with
    /// `Self::deallocate_instance`.
    fn drop(&mut self) {
        // Because `fetch_sub` is already atomic, we do not need to
        // synchronize with other thread.
        if self.strong.fetch_sub(1, atomic::Ordering::Release) != 1 {
            return;
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

        // Now we can deallocate the instance. Note that we don't
        // check the pointer to `Instance` is correctly initialized,
        // but the way `InstanceHandle` creates the
        // `InstanceRef` ensures that.
        unsafe { Self::deallocate_instance(self) };
    }
}

impl MemoryUsage for InstanceRef {
    fn size_of_val(&self, tracker: &mut dyn MemoryUsageTracker) -> usize {
        mem::size_of_val(self) + self.strong.size_of_val(tracker) - mem::size_of_val(&self.strong)
            + self.instance_layout.size_of_val(tracker)
            - mem::size_of_val(&self.instance_layout)
            + self.as_ref().size_of_val(tracker)
            - mem::size_of_val(&self.instance)
    }
}
