use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    ptr,
    rc::Rc,
    sync::{LazyLock, atomic::AtomicU64},
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use corosensei::{Coroutine, CoroutineResult, Yielder, stack::DefaultStack};
use dashmap::DashMap;

use super::entities::function::Function as SysFunction;
use crate::{
    AsStoreAsync, AsStoreMut, AsStoreRef, ForcedStoreInstallGuard, LocalRwLockWriteGuard,
    RuntimeError, Store, StoreAsync, StoreContext, StoreInner, StoreMut, StoreRef, Value,
};
use wasmer_types::StoreId;
use wasmer_vm::ActivationState;

type HostFuture = Pin<Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>> + 'static>>;
type CallResult = Result<Box<[Value]>, RuntimeError>;

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

static ASYNC_CALL_FUTURE_WAKERS: LazyLock<DashMap<StoreId, HashMap<AsyncCallFutureId, Waker>>> =
    LazyLock::new(DashMap::new);

#[allow(clippy::type_complexity)]
pub(crate) struct AsyncCallFuture {
    id: AsyncCallFutureId,

    coroutine: Option<Coroutine<AsyncResume, AsyncYield, CallResult>>,
    pending_store_install: Option<Pin<Box<dyn Future<Output = ForcedStoreInstallGuard>>>>,
    pending_future: Option<HostFuture>,
    next_resume: Option<AsyncResume>,
    result: Option<CallResult>,

    // Store handle we can use to lock the store down
    store: StoreAsync,

    activation: Activation,
}

/// How many coroutine stacks a thread keeps for its next `call_async`s.
const STACK_POOL_CAPACITY: usize = 16;

thread_local! {
    /// Stacks of `call_async` coroutines that ran to completion on this
    /// thread, for its next calls. A stack is a 1 MiB mapping with a guard
    /// page; mapping and unmapping one costs several times what the rest of a
    /// call does.
    ///
    /// This is not `wasmer_vm`'s cache of Wasm stacks: that one keeps a
    /// single stack per thread, which the Wasm frames of a call already take,
    /// sends the rest through a queue shared by all threads, and discards
    /// stacks smaller than the configured Wasm stack size.
    static STACK_POOL: RefCell<Vec<DefaultStack>> = const { RefCell::new(Vec::new()) };
}

/// A stack for a new coroutine: a pooled one if there is any.
fn take_stack() -> DefaultStack {
    // `try_with`: a call can be made while the thread's locals are destroyed.
    STACK_POOL
        .try_with(|pool| pool.borrow_mut().pop())
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Keeps the stack of a coroutine that returned for the thread's next call,
/// or unmaps it if the thread has enough of them.
///
/// Only a coroutine whose function returned gives its stack back. One that
/// was unwound, by a host panic or because its call was dropped while
/// suspended, is dropped with its stack, which is unmapped as before. Those
/// exits are rare and cost far more than a mapping, so reuse is kept to the
/// one exit after which every frame on the stack has returned.
fn give_stack_back(stack: DefaultStack) {
    let _ = STACK_POOL.try_with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.len() < STACK_POOL_CAPACITY {
            pool.push(stack);
        }
    });
}

thread_local! {
    /// The `call_async` coroutine this thread is running on, if any.
    static CURRENT_COROUTINE: Cell<*const CoroutineContext> = const { Cell::new(ptr::null()) };
}

/// The thread-local state that belongs to one `call_async`: kept here while
/// its coroutine is suspended, and exchanged with the thread's by
/// [`AsyncCallFuture::switch_to_coroutine`], the only place where the thread
/// enters or leaves the coroutine.
///
/// Suspended calls can be resumed in any order, so none of this may be left
/// on the thread while the coroutine is not running: another call would run
/// with it, or put its own on top and have it popped out of order.
struct Activation {
    /// The trap handler the signal handler consults and the interrupt
    /// registry's active stores.
    vm: ActivationState,
    /// [`CURRENT_COROUTINE`].
    coroutine: *const CoroutineContext,
}

impl Activation {
    /// # Safety
    /// See [`ActivationState::swap`].
    unsafe fn swap(&mut self) {
        unsafe { self.vm.swap() };
        CURRENT_COROUTINE.with(|cell| self.coroutine = cell.replace(self.coroutine));
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
        let coroutine = Coroutine::with_stack(
            take_stack(),
            move |yielder: &Yielder<AsyncResume, AsyncYield>, resume| {
                assert!(matches!(resume, AsyncResume::Start));

                let ctx_state = CoroutineContext::new(yielder);
                CURRENT_COROUTINE.with(|cell| cell.set(&ctx_state));
                let mut store_mut = AsyncCallStoreMut { store_id };
                function.call(&mut store_mut, &params)
            },
        );

        Self {
            id: NEXT_ASYNC_CALL_FUTURE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            coroutine: Some(coroutine),
            pending_store_install: None,
            pending_future: None,
            next_resume: Some(AsyncResume::Start),
            result: None,
            store,
            activation: Activation {
                vm: ActivationState::new(),
                coroutine: ptr::null(),
            },
        }
    }

    /// Runs the coroutine until it suspends or finishes; or, given no
    /// `resume_arg`, unwinds a suspended coroutine (see `Drop`).
    ///
    /// The coroutine's [`Activation`] is on the thread for exactly as long as
    /// the thread is on the coroutine's stack, and is taken back however the
    /// thread leaves it: by suspending, by returning, or by a host panic
    /// propagating out of `resume`.
    fn switch_to_coroutine(
        &mut self,
        resume_arg: Option<AsyncResume>,
    ) -> Option<CoroutineResult<AsyncYield, CallResult>> {
        struct SwitchBack<'a> {
            activation: &'a mut Activation,
            suspended: bool,
        }
        impl Drop for SwitchBack<'_> {
            fn drop(&mut self) {
                // Safety: the second half of the pair below; the thread is
                // back on the stack it switched from.
                unsafe { self.activation.swap() };
                if !self.suspended {
                    // The coroutine returned or was unwound: nothing may be
                    // left pointing into a stack that is gone.
                    self.activation.coroutine = ptr::null();
                    debug_assert!(self.activation.vm.is_empty());
                }
            }
        }

        // Safety: host code is about to switch to the coroutine that owns the
        // activation, on the thread that runs it; `SwitchBack` swaps again as
        // soon as control is back, also when it comes back by unwinding. The
        // state is not used again once the coroutine is gone.
        unsafe { self.activation.swap() };
        let mut switch_back = SwitchBack {
            activation: &mut self.activation,
            suspended: false,
        };
        let result = match resume_arg {
            Some(resume_arg) => Some(self.coroutine.as_mut().unwrap().resume(resume_arg)),
            None => {
                self.coroutine = None;
                None
            }
        };
        switch_back.suspended = matches!(result, Some(CoroutineResult::Yield(_)));
        drop(switch_back);
        result
    }

    fn remove_from_wakers_list(&self) {
        let mut wakers_entry = match ASYNC_CALL_FUTURE_WAKERS.entry(self.store.store_id()) {
            dashmap::Entry::Occupied(o) => o,
            dashmap::Entry::Vacant(v) => return,
        };

        let mut waker_map_ref = wakers_entry.get_mut();
        waker_map_ref.remove(&self.id);
        if waker_map_ref.is_empty() {
            wakers_entry.remove();
        }
    }
}

impl Future for AsyncCallFuture {
    type Output = Result<Box<[Value]>, RuntimeError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let store_id = self.store.store_id();

            #[cfg(feature = "experimental-host-interrupt")]
            {
                if super::vm::interrupt_registry::is_interrupted(store_id)
                    || self.activation.vm.is_interrupted(store_id)
                {
                    self.remove_from_wakers_list();
                    return Poll::Ready(Err(super::vm::Trap::lib(
                        super::vm::TrapCode::HostInterrupt,
                    )
                    .into()));
                }
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
                self.remove_from_wakers_list();
                return Poll::Ready(self.result.take().expect("polled after completion"));
            }

            {
                let mut wakers_entry = ASYNC_CALL_FUTURE_WAKERS.entry(store_id).or_default();
                wakers_entry.insert(self.id, cx.waker().clone());
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
            match self.switch_to_coroutine(Some(resume_arg)) {
                Some(CoroutineResult::Yield(AsyncYield(fut))) => {
                    self.pending_future = Some(fut);
                }
                Some(CoroutineResult::Return(result)) => {
                    give_stack_back(self.coroutine.take().unwrap().into_stack());
                    self.result = Some(result);
                }
                None => unreachable!("resumed with an argument"),
            }

            // The coroutine has no store context entry of its own: a store it
            // enters synchronously stays on top until that call returns, and
            // cannot suspend in between (see `block_on_host_future`). So the
            // entry installed above is on top again.
            debug_assert!(
                StoreContext::top_is_async(),
                "a call_async coroutine left a store context entry behind"
            );
            // Uninstall the store context to unlock the store after the coroutine
            // yields or returns.
            drop(store_context_guard);
        }
    }
}

impl Drop for AsyncCallFuture {
    fn drop(&mut self) {
        self.remove_from_wakers_list();
        // A call dropped while suspended is cancelled by unwinding its
        // coroutine: host frames run their destructors, guest frames are
        // discarded without running. The unwind happens on the coroutine, so
        // it needs the coroutine's state like any other resumption. The store
        // is not locked for it and nothing on the way may use it. The pending
        // host future is dropped afterwards, with the other fields.
        if self
            .coroutine
            .as_ref()
            .is_some_and(|c| c.started() && !c.done())
        {
            self.switch_to_coroutine(None);
        }
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
        // A store entered synchronously from inside a coroutine (another
        // store, called by a host function) is a sync context too.
        Some(_) if !StoreContext::top_is_async() => run_immediate(future),
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
    let dashmap::Entry::Occupied(entry) = ASYNC_CALL_FUTURE_WAKERS.entry(store_id) else {
        return;
    };

    for waker in entry.get().values() {
        waker.wake_by_ref();
    }
}

/// What host code needs to suspend the coroutine it is running on. It lives
/// on the coroutine's own stack.
struct CoroutineContext {
    yielder: *const Yielder<AsyncResume, AsyncYield>,
}

impl CoroutineContext {
    fn new(yielder: &Yielder<AsyncResume, AsyncYield>) -> Self {
        Self {
            yielder: yielder as *const _,
        }
    }

    fn get_current() -> Option<*const Self> {
        let current = CURRENT_COROUTINE.with(|cell| cell.get());
        (!current.is_null()).then_some(current)
    }

    fn block_on_future(&self, future: HostFuture) -> Result<Vec<Value>, RuntimeError> {
        // The poller takes this coroutine's state off the thread and puts it
        // back before resuming us; see `AsyncCallFuture::switch_to_coroutine`.
        let yielder = unsafe { self.yielder.as_ref().expect("yielder pointer valid") };
        match yielder.suspend(AsyncYield(future)) {
            AsyncResume::HostFutureReady(result) => result,
            AsyncResume::Start => unreachable!("coroutine resumed without start"),
        }
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
mod stack_pool {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use corosensei::stack::Stack;

    use super::*;
    use crate::{Function, FunctionType};

    type Call = Pin<Box<dyn Future<Output = CallResult>>>;

    /// Pending until its flag is set.
    struct Gate(Rc<Cell<bool>>);
    impl Future for Gate {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            if self.0.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    struct Host {
        store: StoreAsync,
        /// Waits for the gate, then returns.
        wait: Function,
        /// Panics.
        panic: Function,
        gate: Rc<Cell<bool>>,
        /// Where on the stack the last `wait` ran.
        ran_at: Rc<Cell<usize>>,
    }

    impl Host {
        fn new() -> Self {
            // Tests can share a thread (`--test-threads=1`).
            STACK_POOL.with(|pool| pool.borrow_mut().clear());
            let mut store = Store::default();
            let gate = Rc::new(Cell::new(false));
            let ran_at = Rc::new(Cell::new(0));
            let ty = FunctionType::new(vec![], vec![]);
            let wait = Function::new_async(&mut store, ty.clone(), {
                let (gate, ran_at) = (gate.clone(), ran_at.clone());
                move |_| {
                    let local = 0u8;
                    ran_at.set(ptr::from_ref(&local) as usize);
                    let gate = gate.clone();
                    async move {
                        Gate(gate).await;
                        Ok(vec![])
                    }
                }
            });
            let panic = Function::new(&mut store, ty, |_| panic!("host function panicked"));
            Self {
                store: store.into_async(),
                wait,
                panic,
                gate,
                ran_at,
            }
        }

        fn call(&self, function: &Function) -> Call {
            Box::pin(function.call_async(&self.store, vec![]))
        }
    }

    fn poll(call: &mut Call) -> Poll<CallResult> {
        call.as_mut()
            .poll(&mut Context::from_waker(&futures::task::noop_waker()))
    }

    /// The address range of each pooled stack.
    fn pooled() -> Vec<std::ops::Range<usize>> {
        STACK_POOL.with(|pool| {
            pool.borrow()
                .iter()
                .map(|stack| stack.limit().get()..stack.base().get())
                .collect()
        })
    }

    #[test]
    fn a_finished_call_leaves_its_stack_to_the_next_one() {
        let host = Host::new();
        host.gate.set(true);
        assert!(matches!(poll(&mut host.call(&host.wait)), Poll::Ready(Ok(_))));
        let first = pooled();
        assert_eq!(first.len(), 1, "the stack of the finished call is kept");
        assert!(first[0].contains(&host.ran_at.get()));

        host.gate.set(false);
        host.ran_at.set(0);
        let mut call = host.call(&host.wait);
        assert!(pooled().is_empty(), "the next call took it");
        assert!(poll(&mut call).is_pending());
        assert!(first[0].contains(&host.ran_at.get()), "and runs on it");
        // A call that overlaps it gets a stack of its own.
        let mut overlapping = host.call(&host.wait);
        assert!(poll(&mut overlapping).is_pending());
        assert!(!first[0].contains(&host.ran_at.get()));

        host.gate.set(true);
        assert!(matches!(poll(&mut call), Poll::Ready(Ok(_))));
        assert!(matches!(poll(&mut overlapping), Poll::Ready(Ok(_))));
        assert_eq!(pooled().len(), 2);
        assert!(pooled().contains(&first[0]));
    }

    #[test]
    fn the_pool_is_bounded() {
        let host = Host::new();
        let mut calls: Vec<Call> = (0..STACK_POOL_CAPACITY + 4)
            .map(|_| host.call(&host.wait))
            .collect();
        for call in &mut calls {
            assert!(poll(call).is_pending());
        }
        assert!(pooled().is_empty());
        host.gate.set(true);
        for call in &mut calls {
            assert!(matches!(poll(call), Poll::Ready(Ok(_))));
        }
        assert_eq!(pooled().len(), STACK_POOL_CAPACITY);
    }

    #[test]
    fn an_unwound_coroutine_does_not_give_its_stack_back() {
        let host = Host::new();

        // Dropped while suspended.
        let mut call = host.call(&host.wait);
        assert!(poll(&mut call).is_pending());
        drop(call);
        assert!(pooled().is_empty(), "dropped while suspended");

        // Unwound by a host panic.
        let mut call = host.call(&host.panic);
        assert!(catch_unwind(AssertUnwindSafe(|| poll(&mut call))).is_err());
        drop(call);
        assert!(pooled().is_empty(), "unwound by a host panic");

        // Never polled.
        drop(host.call(&host.wait));
        assert!(pooled().is_empty(), "never started");

        // The same with a stack that had been pooled: it is not put back.
        host.gate.set(true);
        assert!(matches!(poll(&mut host.call(&host.wait)), Poll::Ready(Ok(_))));
        assert_eq!(pooled().len(), 1);
        host.gate.set(false);
        let mut call = host.call(&host.wait);
        assert!(poll(&mut call).is_pending());
        drop(call);
        let mut call = host.call(&host.panic);
        assert!(catch_unwind(AssertUnwindSafe(|| poll(&mut call))).is_err());
        drop(call);
        assert!(pooled().is_empty());

        // And calls keep working.
        host.gate.set(true);
        assert!(matches!(poll(&mut host.call(&host.wait)), Poll::Ready(Ok(_))));
        assert_eq!(pooled().len(), 1);
    }
}
