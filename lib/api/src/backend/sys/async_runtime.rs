use std::rc::Weak;
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr,
    rc::Rc,
    sync::{Arc, LazyLock, atomic::AtomicU64},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use corosensei::{Coroutine, CoroutineResult, Yielder};
use dashmap::DashMap;

use super::entities::function::Function as SysFunction;
use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, ForcedStoreInstallGuard, LocalRwLock,
    LocalRwLockReadGuard, LocalRwLockWriteGuard, RuntimeError, Store, StoreAsync, StoreContext,
    StoreContextId, StoreInner, StoreMut, StoreRef, SuspendedStoreContexts, Value,
};
use wasmer_types::StoreId;

type HostFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + 'static>>;

pub(crate) fn call_function_async(
    function: SysFunction,
    store: StoreAsync,
    params: Vec<Value>,
) -> AsyncCallFuture {
    AsyncCallFuture::new(function, store, params)
}

struct AsyncYield(HostFuture);

enum AsyncResume {
    Start,
    HostFutureReady(Result<Vec<Value>, RuntimeError>),
}

type AsyncCallFutureId = u64;

static NEXT_ASYNC_CALL_FUTURE_ID: AtomicU64 = AtomicU64::new(0);

static ASYNC_CALL_FUTURE_WAKERS: LazyLock<
    DashMap<StoreId, HashMap<AsyncCallFutureId, Arc<Waker>>>,
> = LazyLock::new(DashMap::new);

#[allow(clippy::type_complexity)]
pub(crate) struct AsyncCallFuture {
    id: AsyncCallFutureId,

    root: Option<Rc<NativeRootOwner>>,
    pending_resume_gate: Option<crate::WriteFuture<()>>,
    pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,
}

/// The stack and its raw TLS snapshots share one physical lifetime. Host futures
/// remain outside this owner, so capturing a weak root cannot create a cycle.
struct NativeRootOwner {
    store: Option<StoreAsync>,
    store_context_id: StoreContextId,
    control: Rc<NativeRootControl>,
    parked: Cell<bool>,
    loan_gate: OnceCell<LocalRwLock<()>>,
    stack: RefCell<NativeRootStack>,
}

#[allow(clippy::type_complexity)]
struct NativeRootStack {
    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    suspended_stores: SuspendedStoreContexts,
    trap_state: RootTrapState,
}

#[derive(Clone)]
struct WeakNativeRoot {
    owner: Weak<NativeRootOwner>,
    id: StoreContextId,
    control: Rc<NativeRootControl>,
}

/// This metadata may outlive the stack. It owns no Store, coroutine or task.
struct NativeRootControl {
    store_id: StoreId,
    installation: StoreContextId,
    retiring: Cell<bool>,
    settled: Cell<bool>,
    retained: Cell<bool>,
    reason: OnceCell<RuntimeError>,
}

thread_local! {
    // A retained original write guard makes this a single-owner association.
    // Weak metadata cannot keep a coroutine, Store or cleanup task alive.
    static RETAINED_ROOTS: RefCell<HashMap<StoreId, Weak<NativeRootControl>>> = RefCell::new(HashMap::new());
    static RETIRING_ROOT: RefCell<Option<Rc<NativeRootControl>>> = const { RefCell::new(None) };
}

// Destructors run without a coroutine yielder or Store authority. Keep only
// the exact cancellation metadata available to managed entry checks.
struct RetirementBoundary {
    previous: Option<Rc<NativeRootControl>>,
    installed: Option<Rc<NativeRootControl>>,
}

impl RetirementBoundary {
    fn enter(control: &Rc<NativeRootControl>) -> Self {
        let installed = (!control.settled.get() && control.retiring.get()).then(|| control.clone());
        let previous = installed.as_ref().and_then(|control| {
            RETIRING_ROOT.with(|current| current.borrow_mut().replace(control.clone()))
        });
        Self {
            previous,
            installed,
        }
    }
}

impl Drop for RetirementBoundary {
    fn drop(&mut self) {
        if let Some(installed) = self.installed.take() {
            let removed = RETIRING_ROOT.with(|current| {
                let mut current = current.borrow_mut();
                assert!(
                    current
                        .as_ref()
                        .is_some_and(|found| Rc::ptr_eq(found, &installed)),
                    "retirement metadata restored out of order"
                );
                std::mem::replace(&mut *current, self.previous.take())
            });
            drop(removed);
        }
    }
}

impl NativeRootControl {
    fn retire(&self) {
        self.reason.get_or_init(|| {
            RuntimeError::host_abort(Box::new(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "native call cancelled",
            )))
        });
        self.retiring.set(true);
    }

    fn error(&self) -> Option<RuntimeError> {
        if self.retiring.get() {
            Some(
                self.reason
                    .get()
                    .expect("retirement reason precedes its flag")
                    .clone(),
            )
        } else {
            None
        }
    }

    fn register(self: &Rc<Self>, stores: &SuspendedStoreContexts) {
        assert_eq!(
            stores.owned_root_identity(),
            Some((self.store_id, self.installation))
        );
        let previous = RETAINED_ROOTS.with(|roots| roots.borrow().get(&self.store_id).cloned());
        if let Some(previous) = previous.and_then(|weak| weak.upgrade()) {
            assert!(
                Rc::ptr_eq(&previous, self),
                "another native root owns this Store"
            );
        }
        let old = RETAINED_ROOTS.with(|roots| {
            roots
                .borrow_mut()
                .insert(self.store_id, Rc::downgrade(self))
        });
        self.retained.set(true);
        drop(old);
    }

    fn unregister(self: &Rc<Self>) {
        if !self.retained.replace(false) {
            return;
        }
        let expected = Rc::downgrade(self);
        let old = RETAINED_ROOTS.with(|roots| {
            let mut roots = roots.borrow_mut();
            assert!(
                roots
                    .get(&self.store_id)
                    .is_some_and(|found| Weak::ptr_eq(found, &expected)),
                "retained Store retirement owner changed"
            );
            roots.remove(&self.store_id)
        });
        drop(old);
    }
}

pub(crate) fn current_retirement_error() -> Option<RuntimeError> {
    if let Some(current) = CoroutineContext::get_current() {
        // SAFETY: installed pointers belong to the currently executing native stack.
        let context = unsafe { &*current };
        if let Some(root) = &context.owner {
            assert_eq!(context.store_context_id, root.control.installation);
            if let Some(error) = root.control.error() {
                return Some(error);
            }
        }
    }
    let control = RETIRING_ROOT.with(|current| current.borrow().clone())?;
    if control.settled.get() {
        None
    } else {
        control.error()
    }
}

pub(crate) fn store_retirement_error(store_id: StoreId) -> Option<RuntimeError> {
    let weak = RETAINED_ROOTS.with(|roots| roots.borrow().get(&store_id).cloned())?;
    let control = weak.upgrade()?;
    assert_eq!(control.store_id, store_id);
    if !control.retained.get() {
        return None;
    }
    control.error()
}

// This token is intentionally not Clone. A future capability may be copied as
// a WeakNativeRoot; strong custody belongs only to an actual outside operation.
struct NativeRootPin {
    _owner: Rc<NativeRootOwner>,
}

impl WeakNativeRoot {
    fn try_pin(&self) -> Option<NativeRootPin> {
        // Do this before touching stack state. A pin on its own coroutine stack
        // would make the owner retain itself, and that stack is currently borrowed.
        if CoroutineContext::get_current()
            .is_some_and(|context| unsafe { &*context }.store_context_id == self.id)
        {
            return None;
        }
        let owner = self.owner.upgrade()?;
        if owner.store_context_id != self.id || !Rc::ptr_eq(&owner.control, &self.control) {
            return None;
        }
        if !owner.parked.get() || owner.control.retiring.get() || owner.control.settled.get() {
            return None;
        }
        Some(NativeRootPin { _owner: owner })
    }
}

/// A weak authority for one still-live derived synchronous borrow. The unit
/// lock owns no Store or coroutine; only an actual outside guard pins the stack.
#[derive(Clone)]
pub(crate) struct NativeStoreLoan {
    root: WeakNativeRoot,
    scope: StoreContextId,
    epoch: Rc<Cell<bool>>,
    gate: LocalRwLock<()>,
    store_id: StoreId,
}

pub(crate) struct NativeLoanReadGuard {
    pointer: *mut StoreInner,
    _gate: LocalRwLockReadGuard<()>,
    _root: NativeRootPin,
}

pub(crate) struct NativeLoanWriteGuard {
    pointer: *mut StoreInner,
    _gate: LocalRwLockWriteGuard<()>,
    _root: NativeRootPin,
}

impl NativeLoanReadGuard {
    pub(crate) fn store(&self) -> &StoreInner {
        // SAFETY: the read gate prevents parent resume and mutable children;
        // the pin retains the original guard and the pointer's exact ancestry.
        unsafe { &*self.pointer }
    }
}

impl NativeLoanWriteGuard {
    pub(crate) fn store(&self) -> &StoreInner {
        // SAFETY: the exclusive gate and root pin cover this shared reborrow.
        unsafe { &*self.pointer }
    }

    pub(crate) fn store_mut(&mut self) -> &mut StoreInner {
        // SAFETY: only this guard has the exclusive gate; the parent is parked.
        unsafe { &mut *self.pointer }
    }
}

impl NativeStoreLoan {
    pub(crate) fn is_live(&self) -> bool {
        self.epoch.get()
            && self
                .root
                .owner
                .upgrade()
                .is_some_and(|owner| !owner.control.retiring.get() && !owner.control.settled.get())
    }

    fn pin(&self) -> Option<NativeRootPin> {
        if !self.epoch.get() {
            return None;
        }
        let pin = self.root.try_pin()?;
        if !pin._owner.parked.get() {
            return None;
        }
        Some(pin)
    }

    fn pointer(&self, pin: &NativeRootPin) -> Option<*mut StoreInner> {
        let stack = pin._owner.stack.borrow();
        stack
            .suspended_stores
            .borrowed_pointer(self.scope, self.store_id)
    }

    pub(crate) fn try_read(&self) -> Option<NativeLoanReadGuard> {
        let root = self.pin()?;
        let gate = self.gate.try_read()?;
        let pointer = self.pointer(&root)?;
        Some(NativeLoanReadGuard {
            pointer,
            _gate: gate,
            _root: root,
        })
    }

    pub(crate) fn try_write(&self) -> Option<NativeLoanWriteGuard> {
        let root = self.pin()?;
        let gate = self.gate.try_write()?;
        let pointer = self.pointer(&root)?;
        Some(NativeLoanWriteGuard {
            pointer,
            _gate: gate,
            _root: root,
        })
    }

    pub(crate) async fn read(&self) -> NativeLoanReadGuard {
        let mut wait = self.gate.read();
        std::future::poll_fn(|cx| {
            let root = self
                .pin()
                .expect("native Store loan is stale, retiring, or still executing");
            match Pin::new(&mut wait).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(gate) => {
                    let pointer = self.pointer(&root).expect("native Store loan scope ended");
                    Poll::Ready(NativeLoanReadGuard {
                        pointer,
                        _gate: gate,
                        _root: root,
                    })
                }
            }
        })
        .await
    }

    pub(crate) async fn write(&self) -> NativeLoanWriteGuard {
        let mut wait = self.gate.write();
        std::future::poll_fn(|cx| {
            let root = self
                .pin()
                .expect("native Store loan is stale, retiring, or still executing");
            match Pin::new(&mut wait).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(gate) => {
                    let pointer = self.pointer(&root).expect("native Store loan scope ended");
                    Poll::Ready(NativeLoanWriteGuard {
                        pointer,
                        _gate: gate,
                        _root: root,
                    })
                }
            }
        })
        .await
    }
}

pub(crate) fn native_coroutine_active() -> bool {
    CoroutineContext::current_root().is_some()
}

pub(crate) fn current_store_loan(store_id: StoreId) -> Option<crate::StoreAsync> {
    let weak = CoroutineContext::current_root()?;
    let root = weak.owner.upgrade()?;
    if root.store().id != store_id || root.control.retiring.get() || root.control.settled.get() {
        return None;
    }
    let (scope, epoch) = StoreContext::borrowed_loan_epoch(root.store_context_id, store_id)?;
    let gate = root.loan_gate.get_or_init(|| LocalRwLock::new(())).clone();
    Some(StoreAsync {
        id: store_id,
        inner: root.store().inner.clone(),
        loan: Some(NativeStoreLoan {
            root: weak,
            scope,
            epoch,
            gate,
            store_id,
        }),
    })
}

impl NativeRootOwner {
    fn store(&self) -> &StoreAsync {
        self.store
            .as_ref()
            .expect("native root Store already released")
    }

    fn has_coroutine(&self) -> bool {
        self.stack.borrow().coroutine.is_some()
    }
}

struct RootResume<'a> {
    root: &'a NativeRootOwner,
    returned: bool,
}

impl Drop for RootResume<'_> {
    fn drop(&mut self) {
        if !self.returned {
            // A panicked resume destroys the engine stack before propagating.
            // Do not admit a new pin while the invalid public future is retained.
            self.root.control.settled.set(true);
        }
    }
}

struct RetainedRootBoundary {
    control: Rc<NativeRootControl>,
    keep: bool,
}

impl Drop for RetainedRootBoundary {
    fn drop(&mut self) {
        if !self.keep {
            self.control.unregister();
        }
    }
}

// We can't use any of the existing AsStoreMut types here, since we keep
// changing the store context underneath us while the coroutine yields.
// To work around it, we use this dummy struct, which just grabs the store
// from the store context. Since we always have a store context installed
// when resuming the coroutine, this is safe in that it can access the store
// through the store context. HOWEVER, references returned from this struct
// CAN NOT BE HELD ACROSS A YIELD POINT. We don't do this anywhere in the
// `Function::call` code.
struct AsyncCallStoreMut {
    store_id: StoreId,
}

impl AsStoreRef for AsyncCallStoreMut {
    fn as_store_ref(&self) -> StoreRef<'_> {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreRef {
                inner: StoreContext::get_current_transient(self.store_id)
                    .as_ref()
                    .unwrap(),
            }
        }
    }
}

impl AsStoreMut for AsyncCallStoreMut {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            StoreMut {
                inner: StoreContext::get_current_transient(self.store_id)
                    .as_mut()
                    .unwrap(),
            }
        }
    }

    fn objects_mut(&mut self) -> &mut crate::StoreObjects {
        // Safety: This is only used with Function::call, which doesn't store
        // the returned reference anywhere, including when calling into WASM
        // code.
        unsafe {
            &mut StoreContext::get_current_transient(self.store_id)
                .as_mut()
                .unwrap()
                .objects
        }
    }
}

impl AsyncCallFuture {
    pub(crate) fn new(function: SysFunction, store: StoreAsync, params: Vec<Value>) -> Self {
        let store_id = store.id;
        let store_context_id = StoreContext::new_installation_id();
        let control = Rc::new(NativeRootControl {
            store_id,
            installation: store_context_id,
            retiring: Cell::new(false),
            settled: Cell::new(false),
            retained: Cell::new(false),
            reason: OnceCell::new(),
        });
        let root = Rc::new_cyclic(|_owner| {
            let weak = WeakNativeRoot {
                owner: _owner.clone(),
                id: store_context_id,
                control: control.clone(),
            };
            let coroutine =
                Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                    assert!(matches!(resume, AsyncResume::Start));
                    let mut ctx_state = CoroutineContext::new(yielder, store_context_id);
                    ctx_state.owner = Some(weak);
                    ctx_state.enter();
                    let result = {
                        let mut store_mut = AsyncCallStoreMut { store_id };
                        function.call_native_root(&mut store_mut, &params, store_context_id)
                    };
                    ctx_state.leave();
                    result
                });
            NativeRootOwner {
                store: Some(store),
                store_context_id,
                control,
                parked: Cell::new(false),
                loan_gate: OnceCell::new(),
                stack: RefCell::new(NativeRootStack {
                    coroutine: Some(coroutine),
                    suspended_stores: SuspendedStoreContexts::default(),
                    trap_state: RootTrapState::default(),
                }),
            }
        });
        Self {
            id: NEXT_ASYNC_CALL_FUTURE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            root: Some(root),
            pending_resume_gate: None,
            pending_store_install: None,
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
        }
    }

    fn root(&self) -> &Rc<NativeRootOwner> {
        self.root.as_ref().expect("native root already released")
    }

    fn store(&self) -> &StoreAsync {
        self.root().store()
    }

    fn remove_from_wakers_list(&self) {
        remove_from_wakers_list(self.store().store_id(), self.id);
    }
}

fn remove_from_wakers_list(store_id: StoreId, id: AsyncCallFutureId) {
    let mut wakers_entry = match ASYNC_CALL_FUTURE_WAKERS.entry(store_id) {
        dashmap::Entry::Occupied(o) => o,
        dashmap::Entry::Vacant(v) => return,
    };

    let mut waker_map_ref = wakers_entry.get_mut();
    let removed = waker_map_ref.remove(&id);
    if waker_map_ref.is_empty() {
        wakers_entry.remove();
    } else {
        drop(wakers_entry);
    }
    // User Waker destructors may reenter this registry.
    drop(removed);
}

fn register_waker(store_id: StoreId, id: AsyncCallFutureId, waker: &Waker) {
    // will_wake compares RawWaker identity without calling its vtable. A poll
    // with an unchanged Waker needs neither a user clone nor an allocation.
    let unchanged = ASYNC_CALL_FUTURE_WAKERS
        .get(&store_id)
        .and_then(|entry| entry.get(&id).map(|old| old.will_wake(waker)))
        .unwrap_or(false);
    if unchanged {
        return;
    }
    let next = Arc::new(waker.clone());
    let previous = {
        let mut entry = ASYNC_CALL_FUTURE_WAKERS.entry(store_id).or_default();
        entry.insert(id, next)
    };
    drop(previous);
}

pub(crate) fn check_async_entry(store: &StoreAsync) -> Result<(), RuntimeError> {
    if let Some(error) = current_retirement_error() {
        return Err(error);
    }
    assert!(
        StoreContext::async_root_admissible(),
        "Function::call_async futures cannot be polled recursively from within another imported function"
    );
    if let Some(loan) = &store.loan {
        return Err(loan.root.control.error().unwrap_or_else(|| {
            RuntimeError::new("cannot start an independent async call from this borrowed Store")
        }));
    }
    Ok(())
}

impl Future for AsyncCallFuture {
    type Output = Result<Box<[Value]>, RuntimeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Err(error) = check_async_entry(self.store()) {
            return Poll::Ready(Err(error));
        }
        loop {
            let store_id = self.store().store_id();

            #[cfg(feature = "experimental-host-interrupt")]
            {
                if super::vm::interrupt_registry::is_interrupted(store_id) {
                    self.remove_from_wakers_list();
                    return Poll::Ready(Err(super::vm::Trap::lib(
                        super::vm::TrapCode::HostInterrupt,
                    )
                    .into()));
                }
            }

            if self.root().has_coroutine() {
                // The pending host future may return before the native stack
                // resumes. Interrupt notification still needs this poll's Waker.
                register_waker(store_id, self.id, cx.waker());
            }

            if let Some(future) = self.pending_future.as_mut() {
                match future.as_mut().poll(cx) {
                    Poll::Ready(result) => {
                        self.pending_future = None;
                        self.next_resume = Some(AsyncResume::HostFutureReady(result));
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }

            // If we're ready, return early
            if !self.root().has_coroutine() {
                self.remove_from_wakers_list();
                return Poll::Ready(self.result.take().expect("polled after completion"));
            }

            // The established local lock owns all waiting/waking behavior. Its
            // write guard excludes every actual outside loan guard from resume.
            if self.pending_resume_gate.is_none()
                && let Some(gate) = self.root().loan_gate.get()
            {
                self.pending_resume_gate = Some(gate.write());
            }
            let resume_gate = if let Some(wait) = self.pending_resume_gate.as_mut() {
                match Pin::new(wait).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(guard) => {
                        self.pending_resume_gate = None;
                        Some(guard)
                    }
                }
            } else {
                None
            };
            let owns_root = self.root().stack.borrow().suspended_stores.owns_root();
            let mut store_context_guard = if owns_root {
                // This is the ORIGINAL write guard, never unlocked/reacquired.
                let root = self.root().clone();
                let mut stack = root.stack.borrow_mut();
                ForcedStoreInstallGuard::restore_borrowed_root(&mut stack.suspended_stores)
            } else {
                if self.pending_store_install.is_none() {
                    self.pending_store_install = Some(Box::pin(install_store_context(
                        self.store().store(),
                        self.root().store_context_id,
                    )));
                }
                match self
                    .pending_store_install
                    .as_mut()
                    .unwrap()
                    .as_mut()
                    .poll(cx)
                {
                    Poll::Ready(guard) => {
                        self.pending_store_install = None;
                        guard
                    }
                    Poll::Pending => return Poll::Pending,
                }
            };
            self.root().parked.set(false);
            // Drop after the stack borrow, but before its original write guard
            // unlocks and wakes a fresh invocation on this same Store.
            let mut retirement = RetainedRootBoundary {
                control: self.root().control.clone(),
                keep: false,
            };

            {
                // This strong reference lives on the executor stack, never on
                // the coroutine it owns. Only the stack fields stay borrowed
                // across resume; identity and retirement remain accessible.
                let root = self.root().clone();
                let mut stack = root.stack.borrow_mut();
                if !owns_root {
                    store_context_guard.restore_suffix(&mut stack.suspended_stores);
                }
                let resume_arg = self.next_resume.take().expect("no resume arg available");
                let trap_boundary = TrapBoundary::enter(std::mem::take(&mut stack.trap_state));
                let coroutine = stack.coroutine.as_mut().unwrap();
                let boundary = CoroutineBoundary::new();
                let mut resume = RootResume {
                    root: &root,
                    returned: false,
                };
                let result = coroutine.resume(resume_arg);
                boundary.returned();
                resume.returned = true;
                let trap_state = trap_boundary.leave();
                match result {
                    CoroutineResult::Yield(AsyncYield(fut)) => {
                        stack.trap_state = trap_state;
                        if StoreContext::can_suspend_borrowed_root(root.store_context_id) {
                            store_context_guard.suspend_borrowed_root(&mut stack.suspended_stores);
                            root.control.register(&stack.suspended_stores);
                            retirement.keep = true;
                        } else {
                            store_context_guard.suspend_suffix(&mut stack.suspended_stores);
                        }
                        self.pending_future = Some(fut);
                        root.parked.set(true);
                    }
                    CoroutineResult::Return(result) => {
                        stack.coroutine = None;
                        root.control.settled.set(true);
                        self.result = Some(result);
                    }
                }
            }

            // Uninstall the store context to unlock the store after the coroutine
            // yields or returns.
            drop(retirement);
            drop(store_context_guard);
            drop(resume_gate);
        }
    }
}

impl Drop for AsyncCallFuture {
    fn drop(&mut self) {
        let store_id = self.store().store_id();
        let root = self.root.take().expect("native root already released");
        if !root.control.settled.get() {
            root.control.retire();
        }
        // A different invocation may be physically active while this one is
        // canceled. None of our destructors may yield through its yielder.
        let _caller = DetachedCoroutineContext::new();
        let _stores = StoreContext::detach_current();
        let _retirement = RetirementBoundary::enter(&root.control);
        let _root = RootTeardown::new(root);
        let owned = DroppedHostState {
            _pending_future: self.pending_future.take(),
            _pending_resume_gate: self.pending_resume_gate.take(),
            _pending_store_install: self.pending_store_install.take(),
            _next_resume: self.next_resume.take(),
            _result: self.result.take(),
        };
        remove_from_wakers_list(store_id, self.id);
        drop(owned);
    }
}

// Keep the root alive while dropping the external host body, and return its
// exact trap state before releasing that strong reference. An outside pin can
// defer final stack destruction to a later, unrelated physical context.
struct RootTeardown {
    root: Rc<NativeRootOwner>,
    boundary: Option<TrapBoundary>,
}

impl RootTeardown {
    fn new(root: Rc<NativeRootOwner>) -> Self {
        let state = std::mem::take(&mut root.stack.borrow_mut().trap_state);
        Self {
            root,
            boundary: Some(TrapBoundary::enter(state)),
        }
    }
}

impl Drop for RootTeardown {
    fn drop(&mut self) {
        let state = self.boundary.take().expect("active root teardown").leave();
        self.root.stack.borrow_mut().trap_state = state;
        // The root field drops after this method, once its saved state is back.
    }
}

impl Drop for NativeRootOwner {
    fn drop(&mut self) {
        let _caller = DetachedCoroutineContext::new();
        let _stores = StoreContext::detach_current();
        let _retirement = RetirementBoundary::enter(&self.control);
        let stack = self.stack.get_mut();
        let _traps = TrapBoundary::enter(std::mem::take(&mut stack.trap_state));
        // Lock waiters may own user Wakers. Empty the field here so their last
        // destructor cannot escape the detached retirement/VM boundaries.
        let loan_gate = self.loan_gate.take();
        let mut owned = DroppedNativeStack {
            _coroutine: stack.coroutine.take(),
            _suspended_stores: std::mem::take(&mut stack.suspended_stores),
            _store: self.store.take(),
        };
        let mut failures = Vec::new();
        if owned._suspended_stores.owns_root() {
            let reason = self
                .control
                .error()
                .expect("borrowed root retires before its final owner");
            let installed =
                ForcedStoreInstallGuard::restore_borrowed_root(&mut owned._suspended_stores);
            let mut retiring = RetiringNativeStack {
                coroutine: owned._coroutine.take(),
                yielded: Vec::new(),
                result: None,
                failures: Vec::new(),
                _settled: SettledNativeRoot(self.control.clone()),
                _retirement: RetainedRootBoundary {
                    control: self.control.clone(),
                    keep: false,
                },
                _installed: installed,
            };
            // Only actual outside guards pin the owner; none remain here.
            // Keep every yielded/returned payload until the stack is dead. A
            // destructor panic must never unlock beneath a suspended frame.
            loop {
                let mut outcome = None;
                let resumed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let boundary = CoroutineBoundary::new();
                    outcome = Some(
                        retiring
                            .coroutine
                            .as_mut()
                            .expect("borrowed coroutine retained")
                            .resume(AsyncResume::HostFutureReady(Err(reason.clone()))),
                    );
                    boundary.returned();
                }));
                if let Err(panic) = resumed {
                    retiring.failures.push(panic);
                }
                if let Some(outcome) = outcome {
                    match outcome {
                        CoroutineResult::Return(result) => retiring.result = Some(result),
                        CoroutineResult::Yield(AsyncYield(future)) => retiring.yielded.push(future),
                    }
                }
                if retiring
                    .coroutine
                    .as_ref()
                    .expect("retiring coroutine retained")
                    .done()
                {
                    break;
                }
            }
            defer_cleanup(&mut retiring.failures, || drop(retiring.coroutine.take()));
            defer_cleanup(&mut retiring.failures, || drop(retiring.result.take()));
            while !retiring.yielded.is_empty() {
                let future = retiring.yielded.remove(0);
                // Never poll or replay an unexpected yield during retirement.
                defer_cleanup(&mut retiring.failures, || drop(future));
            }
            // Asyncify's callback belongs to this finished invocation. Its
            // resources remain owned until the old stack is dead, and it can
            // neither execute nor carry into a fresh root after cancellation.
            defer_cleanup(&mut retiring.failures, || {
                let callback = {
                    let mut wrapper = unsafe { StoreContext::get_current(self.control.store_id) };
                    wrapper.as_mut().inner.on_called.take()
                };
                drop(callback);
            });
            failures = std::mem::take(&mut retiring.failures);
            // The sentinel marks settled, then the exact weak row is removed,
            // then the original guard unlocks. Catch any unlock/Waker panic too.
            defer_cleanup(&mut failures, || drop(retiring));
        }
        defer_cleanup(&mut failures, || drop(owned));
        self.control.settled.set(true);
        defer_cleanup(&mut failures, || drop(loan_gate));
        if !failures.is_empty() {
            let first = failures.remove(0);
            // Every stack/Store/guard has settled before panic payload disposal.
            // As with ordinary Rust cleanup, a repeatedly panicking payload
            // destructor may replace the first panic; no payload is leaked.
            for payload in failures {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)));
            }
            std::panic::resume_unwind(first);
        }
    }
}

type DeferredPanic = Box<dyn std::any::Any + Send>;

fn defer_cleanup(failures: &mut Vec<DeferredPanic>, cleanup: impl FnOnce()) {
    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(cleanup)) {
        failures.push(panic);
    }
}

// Field order is also the unwind fallback: dispose the real stack before any
// owned user payload, mark it settled, remove retirement metadata, then unlock.
#[allow(clippy::type_complexity)]
struct RetiringNativeStack {
    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    yielded: Vec<HostFuture>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,
    failures: Vec<DeferredPanic>,
    _settled: SettledNativeRoot,
    _retirement: RetainedRootBoundary,
    _installed: ForcedStoreInstallGuard,
}

struct SettledNativeRoot(Rc<NativeRootControl>);

impl Drop for SettledNativeRoot {
    fn drop(&mut self) {
        self.0.settled.set(true);
    }
}

struct DroppedHostState {
    _pending_future: Option<HostFuture>,
    _pending_resume_gate: Option<crate::WriteFuture<()>>,
    _pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    _next_resume: Option<AsyncResume>,
    _result: Option<Result<Box<[Value]>, RuntimeError>>,
}

// Fields also drop in this order if a destructor panics. The stack's Store and
// raw Sync snapshots must outlive every native frame during its forced unwind.
#[allow(clippy::type_complexity)]
struct DroppedNativeStack {
    _coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    _suspended_stores: SuspendedStoreContexts,
    _store: Option<StoreAsync>,
}

// The baremetal backend has no stack-local VM trap handlers to exchange.
#[derive(Default)]
struct RootTrapState {
    #[cfg(not(feature = "baremetal"))]
    state: wasmer_vm::SavedTrapState,
    #[cfg(all(unix, feature = "experimental-host-interrupt"))]
    interrupts: wasmer_vm::interrupt_registry::SavedInterruptState,
}

impl RootTrapState {
    unsafe fn replace_current(self) -> Self {
        // No guest code or user callback runs between the interrupt marker and
        // VM handler exchanges. A signal during this host-stack transition is
        // rejected by the handler's Wasm-stack bounds check.
        Self {
            #[cfg(all(unix, feature = "experimental-host-interrupt"))]
            interrupts: unsafe { self.interrupts.replace_current() },
            #[cfg(not(feature = "baremetal"))]
            state: unsafe { self.state.replace_current() },
        }
    }
}

struct TrapBoundary {
    caller: Option<RootTrapState>,
}

impl TrapBoundary {
    fn enter(root: RootTrapState) -> Self {
        // The invocation retains the coroutine and its Store until this
        // boundary is removed, including while its destructors unwind.
        Self {
            caller: Some(unsafe { root.replace_current() }),
        }
    }

    fn leave(mut self) -> RootTrapState {
        // Restore the caller before polling any host future. Only a normal
        // yield retains the returned raw pointers; Return discards them.
        unsafe {
            self.caller
                .take()
                .expect("active trap boundary")
                .replace_current()
        }
    }
}

impl Drop for TrapBoundary {
    fn drop(&mut self) {
        // On panic the engine stack may already be dead. Exchange never
        // dereferences its old pointers, and no token retains them afterward.
        if let Some(caller) = self.caller.take() {
            unsafe { caller.replace_current() };
        }
    }
}

async fn install_store_context(
    store: StoreAsync,
    installation: StoreContextId,
) -> ForcedStoreInstallGuard {
    match unsafe { crate::StoreContext::try_get_current_async(store.id) } {
        crate::GetStoreAsyncGuardResult::NotInstalled => {
            // We always need to acquire a new write lock on the store.
            let store_guard = store.inner.write().await;
            crate::StoreContext::install_async_with_id(store_guard, installation)
        }
        _ => {
            // If we're already in a store context, it is unsafe to reuse
            // the existing store ref since it'll also be accessible from
            // the imported function that tried to poll us, which is a
            // double mutable borrow.
            // Note to people who discover this code: this *would* be safe
            // if we had a separate variation of call_async that just
            // used the existing coroutine context instead of spawning a
            // new coroutine. However, the current call_async always spawns
            // a new coroutine, so we can't allow this; every coroutine
            // needs to own its write lock on the store to make sure there
            // are no overlapping mutable borrows. If this is something
            // you're interested in, feel free to open a GitHub issue outlining
            // your use-case.
            panic!(
                "Function::call_async futures cannot be polled recursively \
                from within another imported function. If you need to await \
                a recursive call_async, consider spawning the future into \
                your async runtime and awaiting the resulting task; \
                e.g. tokio::task::spawn(func.call_async(...)).await"
            );
        }
    }
}

pub enum AsyncRuntimeError {
    YieldOutsideAsyncContext,
    RuntimeError(RuntimeError),
}

pub(crate) fn block_on_host_future<Fut>(future: Fut) -> Result<Vec<Value>, AsyncRuntimeError>
where
    Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
{
    match CoroutineContext::get_current() {
        None => {
            // If there is no async context or we haven't entered it,
            // we can still directly run a future that doesn't block
            // inline.
            run_immediate(future)
        }
        Some(context) => unsafe { context.as_ref().expect("valid context pointer") }
            .block_on_future(Box::pin(future))
            .map_err(AsyncRuntimeError::RuntimeError),
    }
}

// This function will only *wake* pending futures belonging to the given store.
// It is expected that the interrupt happens before this function is called, so
// that the futures can catch and react to `interrupt_registry::is_interrupted`
// correctly.
pub(crate) fn notify_pending_futures_of_interrupt(store_id: StoreId) {
    let wakers: Vec<_> = {
        let Some(entry) = ASYNC_CALL_FUTURE_WAKERS.get(&store_id) else {
            return;
        };
        // Only Arc cloning happens under the entry. RawWaker clone, wake and
        // Drop can run arbitrary code that reenters this same Store's registry.
        entry.values().cloned().collect()
    };
    for waker in wakers {
        waker.wake_by_ref();
    }
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Vec<*const CoroutineContext>> = const { RefCell::new(Vec::new()) };
}

struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
    store_context_id: StoreContextId,
    owner: Option<WeakNativeRoot>,
}

/// Hide a foreign physical coroutine from all cancellation destructors.
struct DetachedCoroutineContext {
    caller: Vec<*const CoroutineContext>,
}

impl DetachedCoroutineContext {
    fn new() -> Self {
        Self {
            caller: CURRENT_CONTEXT.with(|current| std::mem::take(&mut *current.borrow_mut())),
        }
    }
}

impl Drop for DetachedCoroutineContext {
    fn drop(&mut self) {
        let remaining = CURRENT_CONTEXT.with(|current| {
            // Any destructor-created coroutine is owned by that destructor;
            // none may outlive it or replace the caller's exact prefix.
            std::mem::replace(&mut *current.borrow_mut(), std::mem::take(&mut self.caller))
        });
        if !std::thread::panicking() {
            assert!(
                remaining.is_empty(),
                "destructor left a coroutine installed"
            );
        }
    }
}

/// Raw context pointers belong to coroutine stacks. Unwinding a resume destroys
/// those stacks before propagating the panic, so remove only that resume's suffix.
struct CoroutineBoundary {
    prefix: Vec<*const CoroutineContext>,
    returned: bool,
}

impl CoroutineBoundary {
    fn new() -> Self {
        Self {
            prefix: CURRENT_CONTEXT.with(|current| current.borrow().clone()),
            returned: false,
        }
    }

    fn returned(mut self) {
        CURRENT_CONTEXT.with(|current| {
            assert_eq!(
                *current.borrow(),
                self.prefix,
                "coroutine did not restore its context before yielding or returning"
            );
        });
        self.returned = true;
    }
}

impl Drop for CoroutineBoundary {
    fn drop(&mut self) {
        if !self.returned {
            CURRENT_CONTEXT.with(|current| {
                let mut contexts = current.borrow_mut();
                // Never discard a caller whose ancestry has been disturbed by
                // another invariant failure during unwinding.
                if contexts.starts_with(&self.prefix) {
                    contexts.truncate(self.prefix.len());
                }
            });
        }
    }
}

impl CoroutineContext {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>, store_context_id: StoreContextId) -> Self {
        Self {
            yielder: yielder as *const _,
            store_context_id,
            owner: None,
        }
    }

    fn enter(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Push this coroutine on top of the active stack.
            borrow.push(self as *const _);
        })
    }

    // The owning resume boundary handles panic cleanup. A stack-local guard
    // must not pop this context again while corosensei unwinds its stack.
    fn leave(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Pop this coroutine from the active stack.
            assert_eq!(
                borrow.last().copied(),
                Some(self as *const _),
                "Active coroutine stack corrupted"
            );
            borrow.pop();
        });
    }

    fn get_current() -> Option<*const Self> {
        CURRENT_CONTEXT.with(|cell| cell.borrow().last().copied())
    }

    fn current_root() -> Option<WeakNativeRoot> {
        let current = Self::get_current()?;
        unsafe { &*current }.owner.clone()
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
        if let Some(error) = self.owner.as_ref().and_then(|root| root.control.error()) {
            return Err(error);
        }
        if !StoreContext::can_suspend_suffix(self.store_context_id)
            && !StoreContext::can_suspend_borrowed_root(self.store_context_id)
        {
            return run_immediate(future).map_err(|error| match error {
                AsyncRuntimeError::RuntimeError(error) => error,
                AsyncRuntimeError::YieldOutsideAsyncContext => RuntimeError::new(
                    "cannot suspend a native call while a borrowed or incompatible Store context remains installed",
                ),
            });
        }
        // Leave the coroutine context since we're yielding back to the
        // parent stack, and will be inactive until the future is ready.
        self.leave();

        let yielder = unsafe { self.yielder.as_ref().expect("yielder pointer valid") };
        let result = match yielder.suspend(AsyncYield(future)) {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        };

        // Once the future is ready, we restore the current coroutine
        // context.
        self.enter();

        result
    }
}

fn run_immediate(
    future: impl Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
) -> Result<Vec<Value>, AsyncRuntimeError> {
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result.map_err(AsyncRuntimeError::RuntimeError),
        Poll::Pending => Err(AsyncRuntimeError::YieldOutsideAsyncContext),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Function, Instance, Module};

    #[test]
    fn finished_and_panicked_roots_reject_pins_before_the_future_is_dropped() {
        thread_local! {
            static ROOT: RefCell<Option<WeakNativeRoot>> = const { RefCell::new(None) };
        }
        for panics in [false, true] {
            let mut store = Store::default();
            let function = Function::new_typed(&mut store, move || -> i32 {
                let root = CoroutineContext::current_root().unwrap();
                ROOT.with(|slot| assert!(slot.borrow_mut().replace(root).is_none()));
                assert!(!panics, "root lifecycle proof");
                42
            });
            let mut call = Box::pin(AsyncCallFuture::new(
                function.into_sys(),
                store.into_async(),
                vec![],
            ));
            let waker = futures::task::noop_waker();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call.as_mut().poll(&mut Context::from_waker(&waker))
            }));
            assert_eq!(result.is_err(), panics);
            if let Ok(result) = result {
                assert!(matches!(result, Poll::Ready(Ok(_))));
            }
            let root = ROOT.with(|slot| slot.borrow_mut().take().unwrap());
            assert_eq!(root.owner.strong_count(), 1);
            assert!(root.try_pin().is_none());
            drop(call);
            assert_eq!(root.owner.strong_count(), 0);
            assert!(StoreContext::is_empty());
            CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        }
    }

    #[test]
    fn an_external_pin_retains_the_actual_stack_until_its_last_owner_drops() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::an_external_pin_retains_the_actual_stack_until_its_last_owner_drops",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! {
            static ROOT: RefCell<Option<WeakNativeRoot>> = const { RefCell::new(None) };
            static PIN: RefCell<Option<NativeRootPin>> = const { RefCell::new(None) };
        }
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct PendingBody(Probe);
        impl Future for PendingBody {
            type Output = Result<Vec<Value>, RuntimeError>;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                let _ = &self.0;
                Poll::Pending
            }
        }
        let stack_drops = Arc::new(AtomicUsize::new(0));
        let body_drops = Arc::new(AtomicUsize::new(0));
        let continued = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let bridge = Function::new_typed(&mut store, {
            let stack_drops = stack_drops.clone();
            let body_drops = body_drops.clone();
            let continued = continued.clone();
            move || -> Result<i32, RuntimeError> {
                let root = CoroutineContext::current_root().expect("root captured before entry");
                assert!(root.try_pin().is_none(), "a root cannot own its own stack");
                ROOT.with(|slot| assert!(slot.borrow_mut().replace(root).is_none()));
                let _stack = Probe(stack_drops.clone());
                block_on_host_future(PendingBody(Probe(body_drops.clone()))).map_err(|error| {
                    match error {
                        AsyncRuntimeError::RuntimeError(error) => error,
                        AsyncRuntimeError::YieldOutsideAsyncContext => {
                            RuntimeError::new("stack ownership proof did not enter an async root")
                        }
                    }
                })?;
                continued.fetch_add(1, Ordering::SeqCst);
                Ok(42)
            }
        });
        let fresh = Function::new_typed(&mut store, || -> i32 { 9 });
        let store = store.into_async();
        let mut call = Box::pin(AsyncCallFuture::new(
            bridge.into_sys(),
            store.store(),
            vec![],
        ));
        let waker = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        let root = ROOT.with(|slot| slot.borrow_mut().take().unwrap());
        assert_eq!(root.owner.strong_count(), 1, "the stack holds only Weak");
        let pin = root.try_pin().expect("outside operation retains root");
        drop(call);
        assert_eq!(body_drops.load(Ordering::SeqCst), 1);
        assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
        assert!(root.try_pin().is_none(), "retirement rejects a new pin");
        PIN.with(|slot| assert!(slot.borrow_mut().replace(pin).is_none()));

        // Release the last pin from a real foreign root. Its current context
        // and VM trap/interrupt state must survive the deferred final unwind.
        let mut foreign_store = Store::default();
        let release = Function::new_typed(&mut foreign_store, || -> i32 {
            let current = CoroutineContext::get_current().expect("foreign root active");
            drop(PIN.with(|slot| slot.borrow_mut().take().unwrap()));
            assert_eq!(CoroutineContext::get_current(), Some(current));
            42
        });
        let result =
            futures::executor::block_on(release.call_async(&foreign_store.into_async(), vec![]))
                .unwrap();
        assert_eq!(result[0].i32(), Some(42));
        assert_eq!(stack_drops.load(Ordering::SeqCst), 1);
        assert_eq!(body_drops.load(Ordering::SeqCst), 1);
        assert_eq!(continued.load(Ordering::SeqCst), 0);
        assert_eq!(root.owner.strong_count(), 0);
        assert!(root.try_pin().is_none());
        let result = futures::executor::block_on(fresh.call_async(&store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(9));
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    #[test]
    fn a_panicking_retirement_yield_keeps_the_borrowed_stack_owned() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::a_panicking_retirement_yield_keeps_the_borrowed_stack_owned",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! {
            static BASE: RefCell<Option<StoreAsync>> = const { RefCell::new(None) };
        }
        struct StackProbe(Arc<AtomicUsize>);
        impl Drop for StackProbe {
            fn drop(&mut self) {
                BASE.with(|slot| {
                    assert!(
                        slot.borrow().as_ref().unwrap().inner.try_write().is_none(),
                        "original write guard unlocked before borrowed stack destruction"
                    )
                });
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        struct YieldPanic(u32);
        struct PanickingFuture {
            polls: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
            stack: Arc<AtomicUsize>,
        }
        impl Future for PanickingFuture {
            type Output = Result<Vec<Value>, RuntimeError>;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                self.polls.fetch_add(1, Ordering::SeqCst);
                Poll::Pending
            }
        }
        impl Drop for PanickingFuture {
            fn drop(&mut self) {
                assert_eq!(
                    self.stack.load(Ordering::SeqCst),
                    1,
                    "retirement payload dropped before the borrowed stack was dead"
                );
                BASE.with(|slot| {
                    assert!(
                        slot.borrow().as_ref().unwrap().inner.try_write().is_none(),
                        "retirement payload lost the original write guard"
                    )
                });
                self.drops.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(YieldPanic(73));
            }
        }
        struct Parent {
            leaf: Option<crate::TypedFunction<(), i32>>,
            stack: Arc<AtomicUsize>,
            polls: Arc<AtomicUsize>,
            drops: Arc<AtomicUsize>,
        }
        let stack_drops = Arc::new(AtomicUsize::new(0));
        let future_polls = Arc::new(AtomicUsize::new(0));
        let future_drops = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let parent = crate::FunctionEnv::new(
            &mut store,
            Parent {
                leaf: None,
                stack: stack_drops.clone(),
                polls: future_polls.clone(),
                drops: future_drops.clone(),
            },
        );
        let wait = Function::new_async(
            &mut store,
            crate::FunctionType::new([], [wasmer_types::Type::I32]),
            |_| std::future::pending::<Result<Vec<Value>, RuntimeError>>(),
        );
        let bridge = Function::new_typed_with_env(
            &mut store,
            &parent,
            |mut env: crate::FunctionEnvMut<Parent>| -> Result<i32, RuntimeError> {
                let (data, mut store) = env.data_and_store_mut();
                let _stack = StackProbe(data.stack.clone());
                let error = data.leaf.as_ref().unwrap().call(&mut store).unwrap_err();
                assert!(error.is_host_abort());
                let current = CoroutineContext::get_current().expect("borrowed root resumed");
                // Managed import entry refuses a new yield while retiring. Exercise
                // the driver's defensive owned-yield path using this exact root's
                // private yielder, with a valid future whose destructor panics.
                let context = unsafe { &*current };
                context.leave();
                let resumed =
                    unsafe { &*context.yielder }.suspend(AsyncYield(Box::pin(PanickingFuture {
                        polls: data.polls.clone(),
                        drops: data.drops.clone(),
                        stack: data.stack.clone(),
                    })));
                context.enter();
                assert!(
                    matches!(resumed, AsyncResume::HostFutureReady(Err(ref next)) if next.is_host_abort())
                );
                // Even a trusted callback catching cancellation cannot continue Wasm.
                Ok(42)
            },
        );
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "wait" (func $wait (result i32)))
            (import "proof" "bridge" (func $bridge (result i32)))
            (func (export "run") (result i32) call $bridge)
            (func (export "leaf") (result i32) call $wait)
            (func (export "fresh") (result i32) i32.const 9))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "wait" => wait, "bridge" => bridge } },
        )
        .unwrap();
        parent.as_mut(&mut store).leaf =
            Some(instance.exports.get_typed_function(&store, "leaf").unwrap());
        let store = store.into_async();
        BASE.with(|slot| assert!(slot.borrow_mut().replace(store.store()).is_none()));
        let mut call = Box::pin(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&store, vec![]),
        );
        let waker = futures::task::noop_waker();
        assert!(
            call.as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        assert_eq!(stack_drops.load(Ordering::SeqCst), 0);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(call)))
            .expect_err("owned retirement payload panic must propagate after settlement");
        assert_eq!(panic.downcast_ref::<YieldPanic>().unwrap().0, 73);
        assert_eq!(stack_drops.load(Ordering::SeqCst), 1);
        assert_eq!(future_polls.load(Ordering::SeqCst), 0);
        assert_eq!(future_drops.load(Ordering::SeqCst), 1);
        assert!(store.inner.try_write().is_some());
        let fresh = futures::executor::block_on(
            instance
                .exports
                .get_function("fresh")
                .unwrap()
                .call_async(&store, vec![]),
        )
        .unwrap();
        assert_eq!(fresh[0].i32(), Some(9));
        drop(BASE.with(|slot| slot.borrow_mut().take()));
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    #[test]
    fn registry_callbacks_can_reenter_while_the_current_waker_changes() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::registry_callbacks_can_reenter_while_the_current_waker_changes",
        ) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct ReentrantWake {
            store_id: StoreId,
            clones: AtomicUsize,
            wakes: AtomicUsize,
            drops: AtomicUsize,
        }
        impl ReentrantWake {
            fn touch(&self, count: &AtomicUsize) {
                // Every user callback can inspect its own Store's registry.
                // A live DashMap entry around any of these calls deadlocks.
                drop(ASYNC_CALL_FUTURE_WAKERS.get(&self.store_id));
                count.fetch_add(1, Ordering::SeqCst);
            }
            fn waker(self: &Arc<Self>) -> Waker {
                static VTABLE: RawWakerVTable =
                    RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);
                unsafe fn clone_waker(data: *const ()) -> RawWaker {
                    // Borrow the existing owned Arc without consuming its raw
                    // reference; return a separately incremented owned clone.
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<ReentrantWake>())
                    });
                    state.touch(&state.clones);
                    RawWaker::new(Arc::into_raw(Arc::clone(&state)).cast(), &VTABLE)
                }
                unsafe fn wake(data: *const ()) {
                    let state = unsafe { Arc::from_raw(data.cast::<ReentrantWake>()) };
                    state.touch(&state.wakes);
                }
                unsafe fn wake_by_ref(data: *const ()) {
                    let state = std::mem::ManuallyDrop::new(unsafe {
                        Arc::from_raw(data.cast::<ReentrantWake>())
                    });
                    state.touch(&state.wakes);
                }
                unsafe fn drop_waker(data: *const ()) {
                    let state = unsafe { Arc::from_raw(data.cast::<ReentrantWake>()) };
                    state.touch(&state.drops);
                }
                // Each RawWaker owns exactly the Arc cloned here or by its
                // clone vtable entry; wake/drop consume that one reference.
                unsafe {
                    Waker::from_raw(RawWaker::new(Arc::into_raw(self.clone()).cast(), &VTABLE))
                }
            }
        }
        let mut store = Store::default();
        let pending = Function::new_async(&mut store, crate::FunctionType::new([], []), |_| {
            std::future::pending::<Result<Vec<Value>, RuntimeError>>()
        });
        let store = store.into_async();
        let make_state = || {
            Arc::new(ReentrantWake {
                store_id: store.id,
                clones: AtomicUsize::new(0),
                wakes: AtomicUsize::new(0),
                drops: AtomicUsize::new(0),
            })
        };
        let first = make_state();
        let second = make_state();
        let first_waker = first.waker();
        let second_waker = second.waker();
        let mut call = Box::pin(AsyncCallFuture::new(pending.into_sys(), store, vec![]));
        let poll = |call: &mut Pin<Box<AsyncCallFuture>>, waker: &Waker| {
            assert!(
                call.as_mut()
                    .poll(&mut Context::from_waker(waker))
                    .is_pending()
            );
        };
        poll(&mut call, &first_waker);
        assert_eq!(first.clones.load(Ordering::SeqCst), 1);
        poll(&mut call, &first_waker);
        assert_eq!(
            first.clones.load(Ordering::SeqCst),
            1,
            "same identity needs no clone"
        );
        poll(&mut call, &second_waker);
        assert_eq!(second.clones.load(Ordering::SeqCst), 1);
        assert_eq!(first.drops.load(Ordering::SeqCst), 1);
        notify_pending_futures_of_interrupt(second.store_id);
        assert_eq!(first.wakes.load(Ordering::SeqCst), 0);
        assert_eq!(second.wakes.load(Ordering::SeqCst), 1);
        assert_eq!(
            second.clones.load(Ordering::SeqCst),
            1,
            "snapshot clones only Arc"
        );
        drop(call);
        assert_eq!(second.drops.load(Ordering::SeqCst), 1);
        assert!(!ASYNC_CALL_FUTURE_WAKERS.contains_key(&second.store_id));
        drop(first_waker);
        drop(second_waker);
        assert_eq!(first.drops.load(Ordering::SeqCst), 2);
        assert_eq!(second.drops.load(Ordering::SeqCst), 2);
        assert_eq!(Arc::strong_count(&first), 1);
        assert_eq!(Arc::strong_count(&second), 1);
        assert!(StoreContext::is_empty());
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }

    fn runtime_subprocess(test_name: &str) -> bool {
        const CHILD: &str = "WASMER_NATIVE_ASYNC_RUNTIME_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // Like async_sync_borrow, isolate invalid native stack transitions
            // in an owned, bounded child process.
            struct OwnedChild(std::process::Child);
            impl Drop for OwnedChild {
                fn drop(&mut self) {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
            let mut child = OwnedChild(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", test_name, "--nocapture"])
                    .env(CHILD, "1")
                    .spawn()
                    .unwrap(),
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                match child.0.try_wait() {
                    Ok(Some(status)) => {
                        assert!(status.success(), "native runtime proof failed: {status}");
                        return false;
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    result => panic!("native runtime proof did not finish: {result:?}"),
                }
            }
        }

        true
    }

    #[test]
    fn cancellation_destructors_cannot_yield_through_another_root() {
        if !runtime_subprocess(
            "backend::sys::async_runtime::tests::cancellation_destructors_cannot_yield_through_another_root",
        ) {
            return;
        }

        type HiddenCaller = (Function, crate::TypedFunction<(), i32>, StoreAsync);
        thread_local! {
            static HIDDEN_CALLER: RefCell<Option<HiddenCaller>> = const { RefCell::new(None) };
        }
        struct Probe(Rc<std::cell::Cell<usize>>);
        impl Drop for Probe {
            fn drop(&mut self) {
                assert!(CoroutineContext::get_current().is_none());
                assert!(matches!(
                    block_on_host_future(std::future::pending()),
                    Err(AsyncRuntimeError::YieldOutsideAsyncContext)
                ));
                let caller = HIDDEN_CALLER.with(|slot| {
                    slot.borrow().as_ref().map(|(function, typed, store)| {
                        (function.clone(), typed.clone(), store.store())
                    })
                });
                if let Some((function, typed, store)) = caller {
                    // The foreign caller's original write guard is hidden, not
                    // released. A destructor must reject before waiting on it.
                    let mut call = Box::pin(function.call_async(&store, vec![]));
                    let waker = futures::task::noop_waker();
                    let Poll::Ready(Err(error)) =
                        call.as_mut().poll(&mut Context::from_waker(&waker))
                    else {
                        panic!("retirement attempted to wait on its hidden caller");
                    };
                    assert!(error.is_host_abort());
                    let mut call = Box::pin(typed.call_async(&store));
                    let Poll::Ready(Err(error)) =
                        call.as_mut().poll(&mut Context::from_waker(&waker))
                    else {
                        panic!("typed retirement attempted to wait on its hidden caller");
                    };
                    assert!(error.is_host_abort());
                }
                self.0.set(self.0.get() + 1);
            }
        }
        struct PendingBody(Probe);
        impl Future for PendingBody {
            type Output = Result<Vec<Value>, RuntimeError>;
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                let _ = &self.0;
                Poll::Pending
            }
        }
        struct RegistryWake {
            store_id: StoreId,
            drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
            wakes: std::sync::atomic::AtomicUsize,
        }
        impl std::task::Wake for RegistryWake {
            fn wake(self: std::sync::Arc<Self>) {
                self.wakes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        impl Drop for RegistryWake {
            fn drop(&mut self) {
                assert!(CoroutineContext::get_current().is_none());
                // A Waker destructor can reenter the registry for its own key.
                // Removing it must release the DashMap entry before this call.
                assert!(ASYNC_CALL_FUTURE_WAKERS.get(&self.store_id).is_none());
                self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let drops = Rc::new(std::cell::Cell::new(0));
        let mut first_store = Store::default();
        let _data = crate::FunctionEnv::new(&mut first_store, ());
        // This Store-owned closure captures a second destructor probe without
        // requiring a Send implementation for the thread-local test state.
        let store_probe = Probe(drops.clone());
        let _owner = Function::new_async(
            &mut first_store,
            crate::FunctionType::new([], []),
            move |_| {
                let _ = &store_probe;
                async { Ok(vec![]) }
            },
        );
        let count = drops.clone();
        let wait = Function::new_async(
            &mut first_store,
            crate::FunctionType::new([], []),
            move |_| PendingBody(Probe(count.clone())),
        );
        let module = Module::new(
            &first_store,
            r#"(module (import "proof" "wait" (func $wait))
                (func (export "run") call $wait))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut first_store,
            &module,
            &crate::imports! { "proof" => { "wait" => wait } },
        )
        .unwrap();
        let mut first = Box::pin(AsyncCallFuture::new(
            instance
                .exports
                .get_function("run")
                .unwrap()
                .as_sys()
                .clone(),
            first_store.into_async(),
            vec![],
        ));
        let waker_drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let waker = Waker::from(std::sync::Arc::new(RegistryWake {
            store_id: first.store().store_id(),
            drops: waker_drops.clone(),
            wakes: std::sync::atomic::AtomicUsize::new(0),
        }));
        assert!(
            first
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
        drop(waker);
        assert_eq!(drops.get(), 0);

        let first = Rc::new(RefCell::new(Some(first)));
        let mut second_store = Store::default();
        let caller_entries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = caller_entries.clone();
        let fresh = Function::new_typed(&mut second_store, move || -> i32 {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            9
        });
        let cancel = Function::new_async(
            &mut second_store,
            crate::FunctionType::new([], []),
            move |_| {
                let current = CoroutineContext::get_current().expect("second root active");
                drop(first.borrow_mut().take().unwrap());
                assert_eq!(CoroutineContext::get_current(), Some(current));
                async { Ok(vec![]) }
            },
        );
        let module = Module::new(
            &second_store,
            r#"(module (import "proof" "cancel" (func $cancel))
                (func (export "run") (result i32) call $cancel i32.const 42))"#,
        )
        .unwrap();
        let second = Instance::new(
            &mut second_store,
            &module,
            &crate::imports! { "proof" => { "cancel" => cancel } },
        )
        .unwrap();
        let fresh_typed = fresh.typed::<(), i32>(&second_store).unwrap();
        let second_store = second_store.into_async();
        HIDDEN_CALLER.with(|slot| {
            assert!(
                slot.borrow_mut()
                    .replace((fresh.clone(), fresh_typed, second_store.store()))
                    .is_none()
            );
        });
        let result = futures::executor::block_on(
            second
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&second_store, vec![]),
        )
        .unwrap();
        assert_eq!(result[0].i32(), Some(42));
        assert_eq!(drops.get(), 2);
        assert_eq!(waker_drops.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(caller_entries.load(std::sync::atomic::Ordering::SeqCst), 0);
        HIDDEN_CALLER.with(|slot| drop(slot.borrow_mut().take()));
        let result = futures::executor::block_on(fresh.call_async(&second_store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(9));
        assert_eq!(caller_entries.load(std::sync::atomic::Ordering::SeqCst), 1);
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_distinct_store_async_call_rejects_recursive_poll_without_starting() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let entered = Arc::new(AtomicUsize::new(0));
        let mut store = Store::default();
        let count = entered.clone();
        let function = Function::new_typed(&mut store, move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let store = store.into_async();
        let mut call = Box::pin(function.call_async(&store, vec![]));
        let other = Store::default().into_async();
        let root = StoreContext::install_async_with_id(
            other.inner.try_write().unwrap(),
            StoreContext::new_installation_id(),
        );
        let waker = futures::task::noop_waker();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call.as_mut().poll(&mut Context::from_waker(&waker))
            }))
            .is_err()
        );
        assert_eq!(entered.load(Ordering::SeqCst), 0);
        {
            let detached = StoreContext::detach_current();
            assert!(StoreContext::is_empty());
            assert!(!StoreContext::async_root_admissible());
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    call.as_mut().poll(&mut Context::from_waker(&waker))
                }))
                .is_err()
            );
            drop(detached);
            assert_eq!(entered.load(Ordering::SeqCst), 0);
        }
        assert_eq!(
            unsafe { StoreContext::get_current(other.id) }
                .as_ref()
                .objects()
                .id(),
            other.id
        );
        drop(root);
        // Rejection did not poll or poison the exact backend invocation.
        assert!(futures::executor::block_on(call).unwrap().is_empty());
        assert_eq!(entered.load(Ordering::SeqCst), 1);
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_panicked_engine_coroutine_restores_context_before_the_next_call() {
        let mut store = Store::default();
        let panic = Function::new_typed(&mut store, || -> () {
            panic!("synchronous callback panic");
        });
        let ready = Function::new_async(&mut store, crate::FunctionType::new([], []), |_| async {
            Ok(vec![])
        });
        let module = Module::new(
            &store,
            r#"(module
            (import "proof" "panic" (func $panic))
            (import "proof" "ready" (func $ready))
            (func (export "bad") call $panic)
            (func (export "next") (result i32) call $ready i32.const 7))"#,
        )
        .unwrap();
        let instance = Instance::new(
            &mut store,
            &module,
            &crate::imports! { "proof" => { "panic" => panic, "ready" => ready } },
        )
        .unwrap();
        let store = store.into_async();
        let bad = instance.exports.get_function("bad").unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                futures::executor::block_on(bad.call_async(&store, vec![]))
            }))
            .is_err()
        );
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
        let next = instance.exports.get_function("next").unwrap();
        let result = futures::executor::block_on(next.call_async(&store, vec![])).unwrap();
        assert_eq!(result[0].i32(), Some(7));
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
        assert!(StoreContext::is_empty());
    }

    #[test]
    fn a_panicked_nested_coroutine_preserves_its_live_parent_context() {
        let mut parent = Coroutine::new(
            |yielder: &Yielder<AsyncResume, AsyncYield>, _: AsyncResume| {
                let context = CoroutineContext::new(yielder, StoreContext::new_installation_id());
                context.enter();
                let mut child = Coroutine::new(
                    |yielder: &Yielder<AsyncResume, AsyncYield>,
                     _: AsyncResume|
                     -> Result<Box<[Value]>, RuntimeError> {
                        let context =
                            CoroutineContext::new(yielder, StoreContext::new_installation_id());
                        context.enter();
                        panic!("nested coroutine panic");
                    },
                );
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let boundary = CoroutineBoundary::new();
                    let _ = child.resume(AsyncResume::Start);
                    boundary.returned();
                }));
                let retained =
                    CURRENT_CONTEXT.with(|current| *current.borrow() == vec![&context as *const _]);
                context.leave();
                assert!(caught.is_err());
                assert!(retained);
                Ok::<_, RuntimeError>(Vec::<Value>::new().into_boxed_slice())
            },
        );
        let boundary = CoroutineBoundary::new();
        let result = parent.resume(AsyncResume::Start);
        boundary.returned();
        assert!(matches!(result, CoroutineResult::Return(Ok(_))));
        CURRENT_CONTEXT.with(|current| assert!(current.borrow().is_empty()));
    }
}
