use std::{
    any::Any,
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct EnvironmentCell {
    split: AtomicBool,
    contents: UnsafeCell<Box<dyn Any + Send + 'static>>,
}

// SAFETY: the cell has one nonclone VMFunctionEnvironment owner. Ordinary access
// borrows that !Sync owner and rejects an active split. Only a unique, !Sync split
// lease can access the contents while split is set; its allocation remains owned
// even if the environment is removed from its Store or moved to another thread.
unsafe impl Sync for EnvironmentCell {}

/// Underlying FunctionEnvironment used by a `VMFunction`.
pub struct VMFunctionEnvironment {
    cell: Arc<EnvironmentCell>,
    _not_sync: PhantomData<Cell<()>>,
}

/// Owns an environment allocation while its data is lent separately from Store.
/// Ordinary access to the environment is rejected until this lease is dropped.
pub struct VMFunctionEnvironmentLease {
    cell: Arc<EnvironmentCell>,
    _not_sync: PhantomData<Cell<()>>,
}

impl std::fmt::Debug for VMFunctionEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VMFunctionEnvironment")
            .field("contents", &self.cell.contents.get())
            .finish()
    }
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            cell: Arc::new(EnvironmentCell {
                split: AtomicBool::new(false),
                contents: UnsafeCell::new(Box::new(val)),
            }),
            _not_sync: PhantomData,
        }
    }

    fn assert_available(&self) {
        assert!(
            !self.cell.split.load(Ordering::Acquire),
            "function environment data is already lent separately from its Store"
        );
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        self.assert_available();
        // SAFETY: no split lease exists, and this nonclone owner is !Sync.
        unsafe { &**self.cell.contents.get() }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        self.assert_available();
        // SAFETY: this is an exclusive borrow of the sole unleased owner.
        unsafe { &mut **self.cell.contents.get() }
    }

    /// Lends the data independently of the environment's Store slot.
    ///
    /// # Panics
    /// Panics if this environment's data is already lent.
    pub fn lend(&mut self) -> VMFunctionEnvironmentLease {
        assert!(
            self.cell
                .split
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "function environment data is already lent separately from its Store"
        );
        VMFunctionEnvironmentLease {
            cell: self.cell.clone(),
            _not_sync: PhantomData,
        }
    }

    /// Consumes an unleased environment and returns its data.
    ///
    /// # Panics
    /// Panics if the data is lent separately from this environment.
    pub fn into_contents(self) -> Box<dyn Any + Send + 'static> {
        self.assert_available();
        match Arc::try_unwrap(self.cell) {
            Ok(cell) => cell.contents.into_inner(),
            Err(_) => panic!("function environment data lease is still being released"),
        }
    }
}

impl VMFunctionEnvironmentLease {
    #[allow(clippy::should_implement_trait)]
    /// Returns the exact allocation captured when the lease was created.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        // SAFETY: this unique lease excludes all access through the Store slot.
        unsafe { &**self.cell.contents.get() }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the leased allocation.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        // SAFETY: the lease is nonclone and exclusively borrowed here.
        unsafe { &mut **self.cell.contents.get() }
    }
}

impl Drop for VMFunctionEnvironmentLease {
    fn drop(&mut self) {
        self.cell.split.store(false, Ordering::Release);
    }
}
