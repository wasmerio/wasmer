use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use corosensei::{Coroutine, CoroutineResult, Yielder};

use super::entities::function::Function as SysFunction;
use crate::{
    AsStoreMut, AsStoreRef, ForcedStoreInstallGuard, RuntimeError, StoreAsync, StoreContext,
    StoreMut, StoreRef, Value,
};
use wasmer_types::StoreId;

#[cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    not(feature = "baremetal")
))]
pub(crate) use interrupt::notify_pending_futures_of_interrupt;

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

#[allow(clippy::type_complexity)]
pub(crate) struct AsyncCallFuture {
    #[cfg(all(
        unix,
        feature = "experimental-host-interrupt",
        not(feature = "baremetal")
    ))]
    interrupt: interrupt::AsyncInterrupt,

    coroutine: Option<Coroutine<AsyncResume, AsyncYield, Result<Box<[Value]>, RuntimeError>>>,
    pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<Result<Box<[Value]>, RuntimeError>>,

    // Store handle we can use to lock the store down
    store: StoreAsync,
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
        let coroutine =
            Coroutine::new(move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                assert!(matches!(resume, AsyncResume::Start));

                let ctx_state = CoroutineContext::new(yielder);
                ctx_state.enter();
                let result = {
                    let mut store_mut = AsyncCallStoreMut { store_id };
                    function.call(&mut store_mut, &params)
                };
                ctx_state.leave();
                result
            });

        Self {
            #[cfg(all(
                unix,
                feature = "experimental-host-interrupt",
                not(feature = "baremetal")
            ))]
            interrupt: interrupt::AsyncInterrupt::default(),
            coroutine: Some(coroutine),
            pending_store_install: None,
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
            store,
        }
    }

    #[cfg(all(
        unix,
        feature = "experimental-host-interrupt",
        not(feature = "baremetal")
    ))]
    fn drop_coroutine(&mut self) {
        if let Some(coroutine) = self.coroutine.take() {
            let _trap_boundary = self.interrupt.enter_traps();
            drop(coroutine);
        }
    }
}

impl Future for AsyncCallFuture {
    type Output = Result<Box<[Value]>, RuntimeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        #[cfg(all(
            unix,
            feature = "experimental-host-interrupt",
            not(feature = "baremetal")
        ))]
        self.interrupt.update_waker(cx.waker());

        loop {
            #[cfg(all(
                unix,
                feature = "experimental-host-interrupt",
                not(feature = "baremetal")
            ))]
            if self.interrupt.is_interrupted() {
                self.drop_coroutine();
                self.interrupt.clear_waker();
                return Poll::Ready(Err(interrupt::host_interrupt()));
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
            if self.coroutine.is_none() {
                #[cfg(all(
                    unix,
                    feature = "experimental-host-interrupt",
                    not(feature = "baremetal")
                ))]
                {
                    self.interrupt.clear_waker();
                }
                return Poll::Ready(self.result.take().expect("polled after completion"));
            }

            // Start a store installation if not in progress already
            if self.pending_store_install.is_none() {
                self.pending_store_install = Some(Box::pin(install_store_context(StoreAsync {
                    id: self.store.id,
                    inner: self.store.inner.clone(),
                })));
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

            let resume_arg = self.next_resume.take().expect("no resume arg available");
            #[cfg(all(
                unix,
                feature = "experimental-host-interrupt",
                not(feature = "baremetal")
            ))]
            if matches!(resume_arg, AsyncResume::Start) {
                let store_id = self.store.id;
                self.interrupt.enable(store_id, cx.waker());
            }

            let this = &mut *self;
            let resume = || this.coroutine.as_mut().unwrap().resume(resume_arg);
            let result = cfg_select! {
                all(unix, feature = "experimental-host-interrupt", not(feature = "baremetal")) => {
                    this.interrupt.resume(resume)
                }
                _ => {{
                    let mut resume = resume;
                    resume()
                }}
            };
            match result {
                CoroutineResult::Yield(AsyncYield(fut)) => {
                    self.pending_future = Some(fut);
                }
                CoroutineResult::Return(result) => {
                    self.coroutine = None;
                    self.result = Some(result);
                }
            }

            // Uninstall the store context to unlock the store after the coroutine
            // yields or returns.
            drop(store_context_guard);
        }
    }
}

#[cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    not(feature = "baremetal")
))]
impl Drop for AsyncCallFuture {
    fn drop(&mut self) {
        self.drop_coroutine();
    }
}

async fn install_store_context(store: StoreAsync) -> ForcedStoreInstallGuard {
    match unsafe { crate::StoreContext::try_get_current_async(store.id) } {
        crate::GetStoreAsyncGuardResult::NotInstalled => {
            // We always need to acquire a new write lock on the store.
            let store_guard = store.inner.write().await;
            unsafe { crate::StoreContext::install_async(store_guard) }
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

#[cfg(all(
    unix,
    feature = "experimental-host-interrupt",
    not(feature = "baremetal")
))]
mod interrupt {
    use super::{AsStoreRef, AsyncCallStoreMut, CoroutineResult, RuntimeError, StoreId};
    use dashmap::DashMap;
    use std::{
        collections::HashMap,
        sync::{
            Arc, LazyLock,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        task::Waker,
    };
    use wasmer_vm::{SavedTrapState, Trap, TrapCode, interrupt_registry};

    static INTERRUPT_WAKERS: LazyLock<DashMap<StoreId, HashMap<u64, Arc<Waker>>>> =
        LazyLock::new(DashMap::new);

    #[derive(Default)]
    pub(super) struct AsyncInterrupt {
        waker: Option<InterruptWaker>,
        flag: Option<Arc<AtomicBool>>,
        traps: SavedTrapState,
    }

    impl AsyncInterrupt {
        pub(super) fn enable(&mut self, store_id: StoreId, waker: &Waker) {
            // The write lock is installed and the coroutine hasn't borrowed it yet.
            let store = AsyncCallStoreMut { store_id };
            if store.as_store_ref().objects().as_sys().interrupts_enabled() {
                let registration = InterruptWaker::new(store_id);
                registration.register(waker);
                self.waker = Some(registration);
            }
        }

        pub(super) fn update_waker(&self, waker: &Waker) {
            if let Some(registration) = &self.waker {
                // Publish the current waker before observing the interrupt flag.
                registration.register(waker);
            }
        }

        pub(super) fn is_interrupted(&self) -> bool {
            self.flag
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
        }

        pub(super) fn clear_waker(&mut self) {
            self.waker = None;
            self.flag = None;
        }

        pub(super) fn enter_traps(&mut self) -> TrapBoundary {
            TrapBoundary::enter(std::mem::take(&mut self.traps))
        }

        pub(super) fn resume<Y, R>(
            &mut self,
            resume: impl FnOnce() -> CoroutineResult<Y, R>,
        ) -> CoroutineResult<Y, R> {
            let boundary = self.enter_traps();
            let result = resume();
            let state = boundary.leave();
            if matches!(result, CoroutineResult::Yield(_)) {
                self.traps = state;
                if self.flag.is_none()
                    && let Some(registration) = &self.waker
                {
                    // The VM registration stays live on the suspended stack.
                    self.flag = interrupt_registry::interrupt_flag(registration.store);
                }
            }
            result
        }
    }

    pub(super) fn host_interrupt() -> RuntimeError {
        Trap::lib(TrapCode::HostInterrupt).into()
    }

    pub(super) struct TrapBoundary {
        caller: Option<SavedTrapState>,
    }

    impl TrapBoundary {
        fn enter(coroutine: SavedTrapState) -> Self {
            // The coroutine's stack remains alive until it yields or unwinds.
            let caller = unsafe { coroutine.replace_current() };
            Self {
                caller: Some(caller),
            }
        }

        fn leave(mut self) -> SavedTrapState {
            // Restore the caller before polling any host future.
            unsafe { self.caller.take().unwrap().replace_current() }
        }
    }

    impl Drop for TrapBoundary {
        fn drop(&mut self) {
            // On unwind the engine stack may already be dead. Exchanging TLS
            // does not dereference its pointers, and we discard them here.
            if let Some(caller) = self.caller.take() {
                unsafe { caller.replace_current() };
            }
        }
    }

    // This function will only *wake* pending futures belonging to the given store.
    // It is expected that the interrupt happens before this function is called, so
    // that the futures can observe their interrupt flag.
    pub(crate) fn notify_pending_futures_of_interrupt(store_id: StoreId) {
        let Some(entry) = INTERRUPT_WAKERS.get(&store_id) else {
            return;
        };

        let wakers: Vec<_> = entry.values().cloned().collect();
        drop(entry);
        for waker in wakers {
            waker.wake_by_ref();
        }
    }

    struct InterruptWaker {
        store: StoreId,
        id: u64,
    }

    impl InterruptWaker {
        fn new(store: StoreId) -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            Self {
                store,
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            }
        }

        fn register(&self, waker: &Waker) {
            if INTERRUPT_WAKERS
                .get(&self.store)
                .and_then(|entry| entry.get(&self.id).map(|old| old.will_wake(waker)))
                .unwrap_or(false)
            {
                return;
            }
            let waker = Arc::new(waker.clone());
            let old = INTERRUPT_WAKERS
                .entry(self.store)
                .or_default()
                .insert(self.id, waker);
            drop(old);
        }
    }

    impl Drop for InterruptWaker {
        fn drop(&mut self) {
            let dashmap::Entry::Occupied(mut entry) = INTERRUPT_WAKERS.entry(self.store) else {
                return;
            };
            let waker = entry.get_mut().remove(&self.id);
            if entry.get().is_empty() {
                entry.remove();
            } else {
                drop(entry);
            }
            // A waker's destructor can run user code.
            drop(waker);
        }
    }
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Vec<*const CoroutineContext>> = const { RefCell::new(Vec::new()) };
}

struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
}

impl CoroutineContext {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>) -> Self {
        Self {
            yielder: yielder as *const _,
        }
    }

    fn enter(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Push this coroutine on top of the active stack.
            borrow.push(self as *const _);
        })
    }

    // Note: we don't use a drop-style guard here on purpose; if a panic
    // happens while a coroutine is running, CURRENT_CONTEXT will be in
    // an inconsistent state. corosensei will unwind all coroutine stacks
    // anyway, and if we had a guard that would get dropped and try to
    // leave its context, it'd panic again at the assert_eq! below.
    fn leave(&self) {
        CURRENT_CONTEXT.with(|cell| {
            let mut borrow = cell.borrow_mut();

            // Pop this coroutine from the active stack.
            assert_eq!(
                borrow.pop(),
                Some(self as *const _),
                "Active coroutine stack corrupted"
            );
        });
    }

    fn get_current() -> Option<*const Self> {
        CURRENT_CONTEXT.with(|cell| cell.borrow().last().copied())
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
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
