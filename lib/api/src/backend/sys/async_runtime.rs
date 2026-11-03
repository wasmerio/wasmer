#[cfg(test)]
use std::rc::Weak;
use std::{
    cell::{Cell, RefCell},
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
    AsStoreAsync, AsStoreMut, AsStoreRef, ForcedStoreInstallGuard, LocalRwLockWriteGuard,
    RuntimeError, Store, StoreAsync, StoreContext, StoreContextId, StoreInner, StoreMut, StoreRef,
    SuspendedStoreContexts, Value,
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
    retiring: Cell<bool>,
    settled: Cell<bool>,
    stack: RefCell<NativeRootStack>,
}

#[allow(clippy::type_complexity)]
struct NativeRootStack {
    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    suspended_stores: SuspendedStoreContexts,
    trap_state: RootTrapState,
}

#[cfg(test)]
#[derive(Clone)]
struct WeakNativeRoot {
    owner: Weak<NativeRootOwner>,
    id: StoreContextId,
}

// This token is intentionally not Clone. A future capability may be copied as
// a WeakNativeRoot; strong custody belongs only to an actual outside operation.
#[cfg(test)]
struct NativeRootPin {
    _owner: Rc<NativeRootOwner>,
}

#[cfg(test)]
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
        if owner.retiring.get() || owner.settled.get() {
            return None;
        }
        Some(NativeRootPin { _owner: owner })
    }
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
            self.root.settled.set(true);
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
        let root = Rc::new_cyclic(|_owner| {
            #[cfg(test)]
            let weak = WeakNativeRoot {
                owner: _owner.clone(),
                id: store_context_id,
            };
            let coroutine =
                Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                    assert!(matches!(resume, AsyncResume::Start));
                    let ctx_state = CoroutineContext::new(yielder, store_context_id);
                    #[cfg(test)]
                    let ctx_state = {
                        let mut ctx_state = ctx_state;
                        ctx_state.owner = Some(weak);
                        ctx_state
                    };
                    ctx_state.enter();
                    let result = {
                        let mut store_mut = AsyncCallStoreMut { store_id };
                        function.call(&mut store_mut, &params)
                    };
                    ctx_state.leave();
                    result
                });
            NativeRootOwner {
                store: Some(store),
                store_context_id,
                retiring: Cell::new(false),
                settled: Cell::new(false),
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

impl Future for AsyncCallFuture {
    type Output = Result<Box<[Value]>, RuntimeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        assert!(
            StoreContext::is_empty(),
            "Function::call_async futures cannot be polled recursively from within another imported function"
        );
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

            // Start a store installation if not in progress already
            if self.pending_store_install.is_none() {
                self.pending_store_install = Some(Box::pin(install_store_context(
                    StoreAsync {
                        id: self.store().id,
                        inner: self.store().inner.clone(),
                    },
                    self.root().store_context_id,
                )));
            }

            // Acquiring a store lock should be the last step before resuming
            // the coroutine, to minimize the time we hold the lock.
            let store_context_guard = match self
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
            };

            {
                // This strong reference lives on the executor stack, never on
                // the coroutine it owns. Only the stack fields stay borrowed
                // across resume; identity and retirement remain accessible.
                let root = self.root().clone();
                let mut stack = root.stack.borrow_mut();
                store_context_guard.restore_suffix(&mut stack.suspended_stores);
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
                        store_context_guard.suspend_suffix(&mut stack.suspended_stores);
                        self.pending_future = Some(fut);
                    }
                    CoroutineResult::Return(result) => {
                        stack.coroutine = None;
                        root.settled.set(true);
                        self.result = Some(result);
                    }
                }
            }

            // Uninstall the store context to unlock the store after the coroutine
            // yields or returns.
            drop(store_context_guard);
        }
    }
}

impl Drop for AsyncCallFuture {
    fn drop(&mut self) {
        let store_id = self.store().store_id();
        let root = self.root.take().expect("native root already released");
        root.retiring.set(true);
        // A different invocation may be physically active while this one is
        // canceled. None of our destructors may yield through its yielder.
        let _caller = DetachedCoroutineContext::new();
        let _root = RootTeardown::new(root);
        let owned = DroppedHostState {
            _pending_future: self.pending_future.take(),
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
        let stack = self.stack.get_mut();
        let _traps = TrapBoundary::enter(std::mem::take(&mut stack.trap_state));
        let owned = DroppedNativeStack {
            _coroutine: stack.coroutine.take(),
            _suspended_stores: std::mem::take(&mut stack.suspended_stores),
            _store: self.store.take(),
        };
        drop(owned);
        self.settled.set(true);
    }
}

struct DroppedHostState {
    _pending_future: Option<HostFuture>,
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
    #[cfg(test)]
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
            #[cfg(test)]
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

    #[cfg(test)]
    fn current_root() -> Option<WeakNativeRoot> {
        let current = Self::get_current()?;
        unsafe { &*current }.owner.clone()
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
        if !StoreContext::can_suspend_suffix(self.store_context_id) {
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

        struct Probe(Rc<std::cell::Cell<usize>>);
        impl Drop for Probe {
            fn drop(&mut self) {
                assert!(CoroutineContext::get_current().is_none());
                assert!(matches!(
                    block_on_host_future(std::future::pending()),
                    Err(AsyncRuntimeError::YieldOutsideAsyncContext)
                ));
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
        let result = futures::executor::block_on(
            second
                .exports
                .get_function("run")
                .unwrap()
                .call_async(&second_store.into_async(), vec![]),
        )
        .unwrap();
        assert_eq!(result[0].i32(), Some(42));
        assert_eq!(drops.get(), 2);
        assert_eq!(waker_drops.load(std::sync::atomic::Ordering::SeqCst), 1);
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
