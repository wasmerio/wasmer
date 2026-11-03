use std::{
    any::Any,
    cell::{Cell, UnsafeCell},
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::ThreadId,
};

use crate::{StoreHandle, StoreObject, StoreObjects};

#[derive(Default)]
struct FrameState {
    count: usize,
    store: usize,
    thread: Option<ThreadId>,
}

struct EnvironmentCell {
    split: AtomicBool,
    frames: Mutex<FrameState>,
    contents: UnsafeCell<Box<dyn Any + Send + 'static>>,
}

// SAFETY: the nonclone raw owner is !Sync. Raw access rejects both a split
// lease and active frame pins, including after that Send owner leaves Store.
// Trusted API access requires the actual Store borrow and exact cell identity;
// active frames additionally bind it to one Store allocation and owner thread.
// Only a unique !Sync split lease can access T independently of that Store.
unsafe impl Sync for EnvironmentCell {}

/// Underlying FunctionEnvironment used by a VMFunction.
pub struct VMFunctionEnvironment {
    cell: Arc<EnvironmentCell>,
    _not_sync: PhantomData<Cell<()>>,
}

/// Allocation identity only: it neither retains T nor grants data access.
#[derive(Clone)]
pub struct VMFunctionEnvironmentIdentity(Weak<EnvironmentCell>);

/// Allocation custody only. This token supplies no safe data access authority.
pub struct VMFunctionEnvironmentFramePin {
    cell: Arc<EnvironmentCell>,
    _not_send: PhantomData<Rc<()>>,
}

/// Owns an environment allocation while its data is lent separately from Store.
pub struct VMFunctionEnvironmentLease {
    cell: Arc<EnvironmentCell>,
    _not_sync: PhantomData<Cell<()>>,
}

impl std::fmt::Debug for VMFunctionEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("VMFunctionEnvironment")
            .field("contents", &self.cell.contents.get())
            .finish()
    }
}

impl std::fmt::Debug for VMFunctionEnvironmentIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("VMFunctionEnvironmentIdentity")
            .field(&self.0.as_ptr())
            .finish()
    }
}

impl PartialEq for VMFunctionEnvironmentIdentity {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for VMFunctionEnvironmentIdentity {}
impl std::hash::Hash for VMFunctionEnvironmentIdentity {
    fn hash<H: std::hash::Hasher>(&self, hash: &mut H) {
        std::hash::Hash::hash(&self.0.as_ptr(), hash);
    }
}

impl VMFunctionEnvironmentIdentity {
    /// Resolve only an attached owner with this exact allocation identity.
    pub fn lookup<'a>(
        &self,
        handle: &StoreHandle<VMFunctionEnvironment>,
        store: &'a StoreObjects,
    ) -> Option<&'a VMFunctionEnvironment> {
        if handle.store_id() != store.id() {
            return None;
        }
        let owner = VMFunctionEnvironment::list(store).get(handle.internal_handle().index() - 1)?;
        (self.0.as_ptr() == Arc::as_ptr(&owner.cell)).then_some(owner)
    }
}

impl EnvironmentCell {
    fn assert_not_split(&self) {
        assert!(
            !self.split.load(Ordering::Acquire),
            "function environment data is already lent separately from its Store"
        );
    }

    fn assert_raw_access(&self) {
        self.assert_not_split();
        let count = self.frames.lock().unwrap().count;
        assert_eq!(
            count, 0,
            "function environment data is retained by an active import frame"
        );
    }

    fn assert_store_access(&self, store: usize) {
        let (store_matches, thread_matches) = {
            let frames = self.frames.lock().unwrap();
            (
                frames.count == 0 || frames.store == store,
                frames.count == 0 || frames.thread == Some(std::thread::current().id()),
            )
        };
        // A rejected transplanted raw owner must not poison live-frame metadata.
        assert!(
            store_matches,
            "environment frame belongs to another Store allocation"
        );
        assert!(
            thread_matches,
            "environment frame belongs to another thread"
        );
    }

    fn pin(self: &Arc<Self>, store: usize) -> Option<VMFunctionEnvironmentFramePin> {
        let thread = std::thread::current().id();
        let mut frames = self.frames.lock().unwrap();
        if frames.count != 0 && (frames.store != store || frames.thread != Some(thread)) {
            return None;
        }
        let Some(count) = frames.count.checked_add(1) else {
            drop(frames);
            panic!("environment frame count exhausted");
        };
        frames.store = store;
        frames.thread = Some(thread);
        frames.count = count;
        drop(frames);
        Some(VMFunctionEnvironmentFramePin {
            cell: self.clone(),
            _not_send: PhantomData,
        })
    }

    fn lend(self: &Arc<Self>) -> VMFunctionEnvironmentLease {
        assert!(
            self.split
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "function environment data is already lent separately from its Store"
        );
        VMFunctionEnvironmentLease {
            cell: self.clone(),
            _not_sync: PhantomData,
        }
    }
}

impl VMFunctionEnvironment {
    /// Wraps host state owned by one Store slot.
    pub fn new(val: impl Any + Send + 'static) -> Self {
        Self {
            cell: Arc::new(EnvironmentCell {
                split: AtomicBool::new(false),
                frames: Mutex::new(FrameState::default()),
                contents: UnsafeCell::new(Box::new(val)),
            }),
            _not_sync: PhantomData,
        }
    }

    /// Return a weak allocation identity without retaining host state.
    pub fn identity(&self) -> VMFunctionEnvironmentIdentity {
        VMFunctionEnvironmentIdentity(Arc::downgrade(&self.cell))
    }

    #[allow(clippy::should_implement_trait)]
    /// Borrow through the raw owner. Refuses active independent leases/frames.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        self.cell.assert_raw_access();
        // SAFETY: this sole raw owner is !Sync and has no active lease/frame.
        unsafe { &**self.cell.contents.get() }
    }

    #[allow(clippy::should_implement_trait)]
    /// Borrow mutably through the raw owner, refusing active leases or frames.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        self.cell.assert_raw_access();
        // SAFETY: exclusive raw owner borrow, without active lease/frame.
        unsafe { &mut **self.cell.contents.get() }
    }

    /// Lend through the raw owner, refusing active frame pins.
    pub fn lend(&mut self) -> VMFunctionEnvironmentLease {
        self.cell.assert_raw_access();
        self.cell.lend()
    }

    /// Take the data only when no lease or frame retains this allocation.
    ///
    /// # Panics
    /// Panics while an independent split lease or temporary frame retains it.
    pub fn into_contents(self) -> Box<dyn Any + Send + 'static> {
        self.cell.assert_raw_access();
        match Arc::try_unwrap(self.cell) {
            Ok(cell) => cell.contents.into_inner(),
            Err(_) => panic!("function environment allocation is still being released"),
        }
    }

    /// # Safety
    /// The owner is attached to the exact Store allocation at `store`, and the
    /// caller holds its actual Store borrow. The address is compared, never read.
    pub unsafe fn pin_for_store(&self, store: usize) -> Option<VMFunctionEnvironmentFramePin> {
        self.cell.pin(store)
    }

    /// # Safety
    /// The caller holds a shared borrow of the actual Store containing this
    /// owner and validated its exact allocation identity. No detached owner may
    /// use this path to bypass the raw-owner frame fence.
    pub unsafe fn borrow_for_store(&self, store: usize) -> &(dyn Any + Send + 'static) {
        self.cell.assert_not_split();
        self.cell.assert_store_access(store);
        unsafe { &**self.cell.contents.get() }
    }

    /// # Safety
    /// As borrow_for_store, with an exclusive actual Store borrow for this T.
    pub unsafe fn borrow_mut_ptr_for_store(&self, store: usize) -> *mut (dyn Any + Send + 'static) {
        self.cell.assert_not_split();
        self.cell.assert_store_access(store);
        unsafe { &mut **self.cell.contents.get() as *mut (dyn Any + Send + 'static) }
    }

    /// # Safety
    /// Exact attached owner identity and exclusive actual Store borrow are held.
    pub unsafe fn lend_for_store(&self, store: usize) -> VMFunctionEnvironmentLease {
        self.cell.assert_store_access(store);
        self.cell.lend()
    }
}

impl VMFunctionEnvironmentFramePin {
    /// Metadata comparison only, without Store or data access.
    pub fn is_bound_to(&self, store: usize) -> bool {
        let frames = self.cell.frames.lock().unwrap();
        frames.count != 0
            && frames.store == store
            && frames.thread == Some(std::thread::current().id())
    }

    /// Fresh callback admission, including the existing exclusive split fence.
    pub fn can_reborrow(&self, store: usize) -> bool {
        self.is_bound_to(store) && !self.cell.split.load(Ordering::Acquire)
    }

    /// Duplicate only allocation custody, bound to the same actual Store.
    /// # Safety
    /// The actual original Store borrow and its frame remain valid.
    pub unsafe fn duplicate(&self, store: usize) -> Self {
        self.cell
            .pin(store)
            .expect("environment frame allocation changed")
    }

    /// # Safety
    /// Hold the original actual Store borrow; no data reference crosses reentry.
    pub unsafe fn borrow_for_store(&self, store: usize) -> &(dyn Any + Send + 'static) {
        self.cell.assert_not_split();
        self.cell.assert_store_access(store);
        unsafe { &**self.cell.contents.get() }
    }

    /// # Safety
    /// Hold the exclusive original actual Store borrow and T access authority.
    pub unsafe fn borrow_mut_ptr_for_store(&self, store: usize) -> *mut (dyn Any + Send + 'static) {
        self.cell.assert_not_split();
        self.cell.assert_store_access(store);
        unsafe { &mut **self.cell.contents.get() as *mut (dyn Any + Send + 'static) }
    }

    /// # Safety
    /// Hold the exclusive original Store borrow and no other live T reference.
    pub unsafe fn lend_for_store(&self, store: usize) -> VMFunctionEnvironmentLease {
        self.cell.assert_store_access(store);
        self.cell.lend()
    }
}

impl Drop for VMFunctionEnvironmentFramePin {
    fn drop(&mut self) {
        let mut frames = self.cell.frames.lock().unwrap();
        let Some(count) = frames.count.checked_sub(1) else {
            drop(frames);
            panic!("environment frame count underflow");
        };
        frames.count = count;
        if frames.count == 0 {
            frames.store = 0;
            frames.thread = None;
        }
        // The allocation and any final T destructor drop after releasing metadata.
    }
}

impl VMFunctionEnvironmentLease {
    #[allow(clippy::should_implement_trait)]
    /// Borrow the exact cell owned by this unique split lease.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        unsafe { &**self.cell.contents.get() }
    }
    #[allow(clippy::should_implement_trait)]
    /// Mutably borrow the exact cell owned by this unique split lease.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        unsafe { &mut **self.cell.contents.get() }
    }

    /// # Safety
    /// The unique lease is held by the actual original Store borrower.
    pub unsafe fn pin_for_store(&self, store: usize) -> Option<VMFunctionEnvironmentFramePin> {
        self.cell.pin(store)
    }
}

impl Drop for VMFunctionEnvironmentLease {
    fn drop(&mut self) {
        self.cell.split.store(false, Ordering::Release);
    }
}
